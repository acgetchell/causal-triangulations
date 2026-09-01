# Release performance

No release-to-release comparison has been published with the retained artifact workflow yet.

During release preparation, run `just performance-release`. That command measures isolated source states, writes the canonical CSV/provenance pair under
`target/bench-reports/`, reloads and validates the pair, and replaces this file with the generated report. `just performance-doc` can subsequently reproduce
the report without invoking Cargo or creating worktrees.
