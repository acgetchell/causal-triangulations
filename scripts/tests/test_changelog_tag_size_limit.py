"""Tests for changelog_utils.py git tag size limit handling.

Tests the 125KB GitHub tag annotation limit detection and annotated tag creation with
CHANGELOG.md references.

Adapted from the delaunay repository. Uses fully synthetic content because
causal-triangulations has no released versions yet.
"""

import sys
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent.parent))

from changelog_utils import ChangelogUtils

# Synthetic changelog section used as a base for inflation tests.
_SYNTHETIC_CHANGELOG_SECTION = """\
### Added

- **feat: Add new triangulation backend** [`abc1234`](https://github.com/acgetchell/causal-triangulations/commit/abc1234)

  Implements a new geometry backend with improved performance characteristics.

- **feat: Seeded deterministic generation** [`def5678`](https://github.com/acgetchell/causal-triangulations/commit/def5678)

  Adds `from_seeded_points` for reproducible test triangulations.

### Changed

- Refactored error types for better debuggability.
- Upgraded delaunay dependency to v0.7.2.

### Fixed

- Fixed non-determinism in FastKernel-based triangulation generation.
"""


class TestTagSizeLimitHandling:
    """Test suite for git tag size limit handling (125KB GitHub limit).

    For large changelogs, creates annotated tags with a short message referencing CHANGELOG.md.
    """

    def test_oversized_changelog_triggers_reference_message(self) -> None:
        """Oversized changelog content should be replaced with a short CHANGELOG.md reference."""
        base_content = _SYNTHETIC_CHANGELOG_SECTION

        # Force the content over GitHub's 125KB annotated-tag message limit.
        max_tag_size = 125_000
        base_size = len(base_content.encode("utf-8"))
        assert base_size > 0

        repeats = (max_tag_size // base_size) + 2
        oversized_content = "\n\n".join([base_content] * repeats)
        assert len(oversized_content.encode("utf-8")) > max_tag_size

        # Patch extraction and path lookup so the test is isolated from the filesystem.
        with (
            patch.object(ChangelogUtils, "find_changelog_path", return_value="CHANGELOG.md"),
            patch.object(ChangelogUtils, "extract_changelog_section", return_value=oversized_content),
        ):
            tag_message, is_truncated = ChangelogUtils._get_changelog_content("v0.1.0")

        assert is_truncated is True, "Large changelog should be truncated"
        assert "See full changelog" in tag_message, "Should contain CHANGELOG.md reference"
        assert "github.com/acgetchell/causal-triangulations" in tag_message, "Should contain GitHub link"
        assert len(tag_message) < 1000, "Reference message should be short"

    def test_small_changelog_within_limit(self) -> None:
        """Test that small changelog content is returned in full."""
        small_content = _SYNTHETIC_CHANGELOG_SECTION

        with (
            patch.object(ChangelogUtils, "find_changelog_path", return_value="CHANGELOG.md"),
            patch.object(ChangelogUtils, "extract_changelog_section", return_value=small_content),
        ):
            tag_message, is_truncated = ChangelogUtils._get_changelog_content("v0.1.0")

        assert is_truncated is False, "Small changelog should not be truncated"
        assert tag_message == small_content, "Should return full content when under limit"

    @patch("changelog_utils.run_git_command_with_input")
    def test_create_tag_with_message_truncated(self, mock_run_git_with_input) -> None:
        """Test annotated tag with reference message for oversized changelogs."""
        ref_message = "Version 0.1.0\n\nSee full changelog in CHANGELOG.md"
        ChangelogUtils._create_tag_with_message("v0.1.0", ref_message, is_truncated=True)

        # Should still create annotated tag with reference message
        mock_run_git_with_input.assert_called_once_with(["tag", "-a", "v0.1.0", "-F", "-"], input_data=ref_message)

    @patch("changelog_utils.run_git_command_with_input")
    def test_create_tag_with_message_normal(self, mock_run_git_with_input) -> None:
        """Test annotated tag creation with full message for normal-sized changelogs."""
        tag_message = "Version 1.0.0\n\n- Feature 1\n- Feature 2"

        ChangelogUtils._create_tag_with_message("v1.0.0", tag_message, is_truncated=False)

        # Should call git tag with -a flag and full message from stdin
        mock_run_git_with_input.assert_called_once_with(["tag", "-a", "v1.0.0", "-F", "-"], input_data=tag_message)

    @patch("builtins.print")
    def test_show_success_message_truncated_still_uses_notes_from_tag(self, mock_print) -> None:
        """Test success message for truncated changelog still uses --notes-from-tag."""
        ChangelogUtils._show_success_message("v0.1.0", is_truncated=True)

        # Collect all print calls
        print_calls = [str(call.args[0]) if call.args else "" for call in mock_print.call_args_list]
        all_output = "\n".join(print_calls)

        # Should mention the tag was created
        assert "Successfully created tag" in all_output

        # Should still use --notes-from-tag (works with reference message)
        assert "--notes-from-tag" in all_output

        # Should note that it references CHANGELOG.md
        assert "references CHANGELOG.md" in all_output

    @patch("builtins.print")
    def test_show_success_message_normal_uses_notes_from_tag(self, mock_print) -> None:
        """Test success message for normal changelog uses --notes-from-tag flag."""
        ChangelogUtils._show_success_message("v1.0.0", is_truncated=False)

        # Collect all print calls
        print_calls = [str(call.args[0]) if call.args else "" for call in mock_print.call_args_list]
        all_output = "\n".join(print_calls)

        # Should mention the tag was created
        assert "Successfully created tag" in all_output

        # Should use --notes-from-tag
        assert "--notes-from-tag" in all_output

        # Should NOT have truncation warning
        assert "references CHANGELOG.md" not in all_output

    @patch("changelog_utils.ChangelogUtils.validate_git_repo")
    @patch("changelog_utils.ChangelogUtils.validate_semver")
    @patch("changelog_utils.ChangelogUtils._handle_existing_tag")
    @patch("changelog_utils.ChangelogUtils._get_changelog_content")
    @patch("changelog_utils.ChangelogUtils._check_git_config")
    @patch("changelog_utils.ChangelogUtils._create_tag_with_message")
    @patch("changelog_utils.ChangelogUtils._show_success_message")
    def test_create_git_tag_full_workflow_large_changelog(  # noqa: PLR0913
        self,
        mock_show_success,
        mock_create_tag,
        mock_check_git_config,
        mock_get_changelog,
        mock_handle_existing,
        mock_validate_semver,
        mock_validate_git_repo,
    ) -> None:
        """Test full workflow for creating tag with large changelog."""
        # Mock large changelog that exceeds limit (returns reference message)
        ref_message = "Version 0.1.0\n\nSee full changelog in CHANGELOG.md"
        mock_get_changelog.return_value = (ref_message, True)  # Reference message, truncated=True

        ChangelogUtils.create_git_tag("v0.1.0", force_recreate=False)

        # Verify workflow steps
        mock_validate_git_repo.assert_called_once()
        mock_validate_semver.assert_called_once_with("v0.1.0")
        mock_handle_existing.assert_called_once_with("v0.1.0", force_recreate=False)
        mock_get_changelog.assert_called_once_with("v0.1.0")

        # Should still check git config (for annotated tag)
        mock_check_git_config.assert_called_once()

        # Should create annotated tag with reference message
        mock_create_tag.assert_called_once_with("v0.1.0", ref_message, is_truncated=True)
        mock_show_success.assert_called_once_with("v0.1.0", is_truncated=True)
