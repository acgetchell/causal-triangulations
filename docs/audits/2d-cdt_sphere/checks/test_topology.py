"""Known surfaces and counterexamples validate the independent census."""

import hashlib
import json
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from .topology import adjacency_census, audit, census, graph, read_export, write_report

if TYPE_CHECKING:
    from .topology import Face

EVIDENCE = Path(__file__).resolve().parent.parent / "evidence"
TETRAHEDRON: list[Face] = [(0, 1, 2), (0, 1, 3), (0, 2, 3), (1, 2, 3)]


def grid(*, periodic: bool = True, twist: bool = False) -> tuple[int, list[Face]]:
    """A cylinder, torus, or Klein bottle, depending on the last-row gluing."""
    width, height = 5, 4
    rows = height if periodic else height + 1

    def label(row: int, col: int) -> int:
        if periodic and row == height:
            row = 0
            col = -col if twist else col
        return row * width + col % width

    faces: list[Face] = []
    for row in range(height):
        for col in range(width):
            a, b = label(row, col), label(row, col + 1)
            c, d = label(row + 1, col), label(row + 1, col + 1)
            faces.extend([(a, b, c), (b, c, d)])
    return rows * width, faces


@pytest.mark.parametrize(
    ("vertices", "faces", "chi", "boundary", "orientable"),
    [
        (4, TETRAHEDRON, 2, 0, True),
        (3, [(0, 1, 2)], 1, 1, True),
        (*grid(periodic=False), 0, 2, True),
        (*grid(), 0, 0, True),
        (*grid(twist=True), 0, 0, False),
    ],
)
def test_known_surfaces(vertices: int, faces: list[Face], chi: int, boundary: int, orientable: bool) -> None:
    result = census(vertices, faces)
    assert result["chi"] == chi
    assert result["boundary_components"] == boundary
    assert result["components"] == 1
    assert result["manifold"] is True
    assert result["orientable"] is orientable
    assert result["closed"] is (boundary == 0)


def test_disconnected_spheres() -> None:
    faces = TETRAHEDRON + [(a + 4, b + 4, c + 4) for a, b, c in TETRAHEDRON]
    result = census(8, faces)
    assert result["components"] == 2
    assert result["chi"] == 4
    assert result["manifold"] is True


def test_pinched_vertex_has_closed_edges_but_is_not_manifold() -> None:
    other = [(0, 4, 5), (0, 4, 6), (0, 5, 6), (4, 5, 6)]
    result = census(7, TETRAHEDRON + other)
    assert result["edge_incidence"] == {"2": 12}
    assert result["vertex_links"] == {"cycle": 6, "invalid": 1}
    assert result["manifold"] is False
    assert result["orientable"] is None


def test_three_faces_on_one_edge() -> None:
    result = census(5, [(0, 1, 2), (0, 1, 3), (0, 1, 4)])
    assert result["edge_incidence"] == {"1": 6, "3": 1}
    assert result["manifold"] is False


def test_isolated_vertex() -> None:
    result = census(5, TETRAHEDRON)
    assert result["components"] == 2
    assert result["manifold"] is False


@pytest.mark.parametrize("faces", [[(0, 0, 1)], [(0, 1, 2), (2, 1, 0)]])
def test_nonsimplicial_input_is_rejected(faces: list[Face]) -> None:
    with pytest.raises(ValueError, match="degenerate or duplicate"):
        census(3, faces)


def test_directed_adjacency_is_not_symmetric_boundary() -> None:
    result = adjacency_census([[1], []], graph(range(2), [(0, 1)]))
    assert result["missing_arcs"] == [[1, 0]]
    assert result["nonreciprocal_arcs"] == [[0, 1]]


def test_export_parser_rejects_truncation(tmp_path: Path) -> None:
    path = tmp_path / "broken.dat"
    path.write_text((EVIDENCE / "true-initial.dat").read_text(encoding="utf-8").rsplit("\n", 2)[0], encoding="utf-8")
    with pytest.raises(ValueError, match="triangle records"):
        read_export(path)


def copy_snapshot(directory: Path) -> Path:
    """Copy retained inputs so negative controls cannot modify the evidence."""
    path = directory / "true-initial.dat"
    for suffix in (".dat", ".json"):
        path.with_suffix(suffix).write_bytes((EVIDENCE / f"true-initial{suffix}").read_bytes())
    return path


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("sphere", "false", "sphere must be a boolean"),
        ("move_counts_reject_add_delete_flip", [0, -1, 0, 0], "nonnegative integers"),
        ("move_counts_reject_add_delete_flip", [0, 0, 0], "reject/add/delete/flip"),
        ("slice_sizes", [999, 3, 3, 3], "disagree"),
        ("slice_sizes", [True, 3, 3, 3], "nonnegative integers"),
        ("vertex_neighbors", [[False]], "nonnegative integers"),
        ("triangle_neighbors", [[999]], "out-of-range"),
        ("links", [[0, 1, 2]], "link-pair shape"),
    ],
)
def test_sidecar_rejects_invalid_metadata(tmp_path: Path, field: str, value: object, message: str) -> None:
    path = copy_snapshot(tmp_path)
    sidecar = path.with_suffix(".json")
    data = json.loads(sidecar.read_text(encoding="utf-8"))
    data[field] = value
    sidecar.write_text(json.dumps(data), encoding="utf-8")
    with pytest.raises(ValueError, match=message):
        audit(path)


def test_export_parser_rejects_negative_time_labels(tmp_path: Path) -> None:
    path = copy_snapshot(tmp_path)
    tokens = path.read_text(encoding="utf-8").split()
    vertices = int(tokens[0])
    tokens[1 : vertices + 1] = ["-1"] * vertices
    path.write_text("\n".join(tokens), encoding="utf-8")
    with pytest.raises(ValueError, match="negative time label"):
        audit(path)


def test_stored_pointer_negative_control_preserves_independent_census(tmp_path: Path) -> None:
    path = copy_snapshot(tmp_path)
    expected = audit(path)
    tokens = path.read_text(encoding="utf-8").split()
    tokens[int(tokens[0]) + 6] = "0"  # Triangle zero cannot neighbor itself across its first edge.
    path.write_text("\n".join(tokens), encoding="utf-8")
    expected["stored_pointer_errors"] = [[0, 0]]
    assert audit(path) == expected


def test_reconstructed_link_negative_control_preserves_independent_census(tmp_path: Path) -> None:
    path = copy_snapshot(tmp_path)
    expected = audit(path)
    sidecar = path.with_suffix(".json")
    data = json.loads(sidecar.read_text(encoding="utf-8"))
    data["links"].pop()
    sidecar.write_text(json.dumps(data), encoding="utf-8")
    expected["reconstructed_links_match"] = False
    assert audit(path) == expected


@pytest.mark.parametrize("kind", ["direct", "symlink", "hardlink"])
@pytest.mark.parametrize("suffix", [".dat", ".json"])
def test_report_rejects_input_aliases(tmp_path: Path, kind: str, suffix: str) -> None:
    path = copy_snapshot(tmp_path).with_suffix(suffix)
    original = path.read_bytes()
    output = tmp_path / "report.json"
    if kind == "symlink":
        output.symlink_to(path)
    elif kind == "hardlink":
        output.hardlink_to(path)
    else:
        output = path
    with pytest.raises(ValueError, match="aliases a snapshot input"):
        write_report(tmp_path, output)
    assert path.read_bytes() == original


def test_report_replacement_failure_preserves_prior_report(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    copy_snapshot(tmp_path)
    output = tmp_path / "report.json"
    output.write_bytes(b"prior complete report\n")
    before = {path.name: path.read_bytes() for path in tmp_path.iterdir()}

    def fail_replace(_source: Path, _destination: Path) -> None:
        message = "injected replacement failure"
        raise OSError(message)

    monkeypatch.setattr(Path, "replace", fail_replace)
    with pytest.raises(OSError, match="injected replacement failure"):
        write_report(tmp_path, output)
    assert {path.name: path.read_bytes() for path in tmp_path.iterdir()} == before


def test_report_publishes_complete_utf8_json(tmp_path: Path) -> None:
    path = copy_snapshot(tmp_path)
    output = tmp_path / "report.json"
    write_report(tmp_path, output)
    assert output.read_bytes().endswith(b"\n")
    assert b"\r" not in output.read_bytes()
    assert json.loads(output.read_text(encoding="utf-8")) == {path.stem: audit(path)}
    assert not list(tmp_path.glob(".report.json.*"))


def test_retained_census_and_round_trips() -> None:
    expected = json.loads((EVIDENCE / "census.json").read_text(encoding="utf-8"))
    assert len(expected) == 7
    for name, result in expected.items():
        assert audit(EVIDENCE / f"{name}.dat") == result
        surface = result["complex"]
        assert (surface["chi"], surface["components"], surface["boundary_components"]) == (0, 1, 0)
        assert surface["closed"]
        assert surface["orientable"]
        assert surface["edge_incidence"] == {"2": surface["N1"]}
        assert surface["vertex_links"] == {"cycle": surface["N0"]}
        assert result["reconstructed_links_match"]
        assert result["stored_pointer_errors"] == []
    for mode in ("false", "true"):
        evolved = expected[f"{mode}-evolved"]
        counts = evolved["move_counts_reject_add_delete_flip"]
        assert sum(counts) == 4096
        assert all(count > 0 for count in counts[1:])
        imported = expected[f"import-{mode}-initial"]
        assert imported["complex"] == evolved["complex"]
        assert imported["observable_vertices"] == evolved["observable_vertices"]
        assert imported["observable_triangles"] == evolved["observable_triangles"]
    assert expected["true-initial"]["observable_triangles"]["out_degree"] == {"2": 6, "3": 18}
    assert len(expected["true-initial"]["observable_triangles"]["nonreciprocal_arcs"]) == 6
    assert expected["import-gap-initial"]["nonadjacent_time_edges"]


def test_evidence_digests_and_input_identity() -> None:
    root = EVIDENCE.parent
    for line in (root / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        expected, relative = line.split(maxsplit=1)
        assert hashlib.sha256((root / relative).read_bytes()).hexdigest() == expected
    config = json.loads((root / "config.json").read_text(encoding="utf-8"))
    assert hashlib.sha256((root / "source.tar.gz").read_bytes()).hexdigest() == config["source_archive_sha256"]
    assert (EVIDENCE / "false-initial.dat").read_bytes() == (EVIDENCE / "true-initial.dat").read_bytes()
    original = read_export(EVIDENCE / "true-initial.dat")
    gap = read_export(EVIDENCE / "gap-input.dat")
    assert gap.faces == original.faces
    assert gap.neighbors == original.neighbors
    mapping = {0: 0, 1: 1, 2: 3, 3: 4}
    assert gap.times == [mapping[time] for time in original.times]
