"""Count simplices and test surface links from raw exported vertex triples.

No upstream reconstruction routine or geometric embedding is used by the census.
Neighbor pointers and observable lists are checked against the resulting complex.
"""

import argparse
import json
from collections import Counter, defaultdict
from dataclasses import dataclass
from itertools import combinations
from pathlib import Path
from typing import TYPE_CHECKING, TypedDict, cast

if TYPE_CHECKING:
    from collections.abc import Iterable

type Edge = tuple[int, int]
type Face = tuple[int, int, int]
type Graph = dict[int, set[int]]


class Observation(TypedDict):
    """Sidecar emitted by harness.cpp in the export's dense label order."""

    sphere: bool
    move_counts_reject_add_delete_flip: list[int]
    slice_sizes: list[int]
    vertex_neighbors: list[list[int]]
    triangle_neighbors: list[list[int]]
    links: list[list[int]]


@dataclass
class Complex:
    """Raw upstream export, including pointers only for consistency checks."""

    times: list[int]
    faces: list[Face]
    neighbors: list[Face]


def read_export(path: Path) -> Complex:
    """Reject truncated records, mismatched counts and out-of-range labels."""
    tokens = [int(value) for value in path.read_text(encoding="utf-8").split()]
    if not tokens or tokens[0] <= 0:
        message = "missing positive vertex count"
        raise ValueError(message)
    n0 = tokens[0]
    if len(tokens) < n0 + 4 or tokens[n0 + 1] != n0:
        message = "vertex count trailer mismatch"
        raise ValueError(message)
    n2 = tokens[n0 + 2]
    if n2 <= 0 or len(tokens) != n0 + 4 + 6 * n2 or tokens[-1] != n2:
        message = "triangle records or count trailer mismatch"
        raise ValueError(message)
    faces: list[Face] = []
    neighbors: list[Face] = []
    for offset in range(n0 + 3, len(tokens) - 1, 6):
        a, b, c, left, right, center = tokens[offset : offset + 6]
        if not all(0 <= v < n0 for v in (a, b, c)) or not all(0 <= t < n2 for t in (left, right, center)):
            message = "out-of-range simplex label"
            raise ValueError(message)
        faces.append((a, b, c))
        neighbors.append((left, right, center))
    return Complex(tokens[1 : n0 + 1], faces, neighbors)


def edge(a: int, b: int) -> Edge:
    """Canonical unoriented edge."""
    return (min(a, b), max(a, b))


def graph(vertices: Iterable[int], edges: Iterable[Edge]) -> Graph:
    """Undirected adjacency retaining isolated vertices."""
    result: Graph = {v: set() for v in vertices}
    for a, b in edges:
        result[a].add(b)
        result[b].add(a)
    return result


def components(adjacency: Graph) -> int:
    """Count components without assuming a surface or connected input."""
    remaining = set(adjacency)
    count = 0
    while remaining:
        frontier = [remaining.pop()]
        count += 1
        while frontier:
            for neighbor in adjacency[frontier.pop()] & remaining:
                remaining.remove(neighbor)
                frontier.append(neighbor)
    return count


def histogram(values: Iterable[int | str]) -> dict[str, int]:
    """Stable, JSON-compatible histogram."""
    return dict(sorted(Counter(str(value) for value in values).items()))


def incidence(faces: list[Face]) -> dict[Edge, list[int]]:
    """Derive all edges from triangles; duplicate faces are not simplicial."""
    if any(len(set(face)) != 3 for face in faces) or len({tuple(sorted(f)) for f in faces}) != len(faces):
        message = "degenerate or duplicate triangle"
        raise ValueError(message)
    result: dict[Edge, list[int]] = defaultdict(list)
    for index, face in enumerate(faces):
        for a, b in combinations(face, 2):
            result[edge(a, b)].append(index)
    return dict(result)


def link_kind(vertex: int, faces: list[Face]) -> str:
    """A surface vertex link is one simple cycle or one simple boundary path."""
    edges: list[Edge] = []
    for face in faces:
        if vertex in face:
            a, b = [v for v in face if v != vertex]
            edges.append(edge(a, b))
    vertices = {v for pair in edges for v in pair}
    adjacency = graph(vertices, edges)
    if not edges or len(set(edges)) != len(edges) or components(adjacency) != 1:
        return "invalid"
    degrees = Counter(len(neighbors) for neighbors in adjacency.values())
    if set(degrees) == {2}:
        return "cycle"
    if degrees[1] == 2 and set(degrees) <= {1, 2}:
        return "path"
    return "invalid"


def orientable(faces: list[Face], edges: dict[Edge, list[int]]) -> bool:
    """Solve opposite directed-edge orientation constraints across each face pair."""
    directions: dict[tuple[int, Edge], int] = {}
    for index, (a, b, c) in enumerate(faces):
        for x, y in ((a, b), (b, c), (c, a)):
            directions[index, edge(x, y)] = 1 if x < y else -1
    constraints: dict[int, list[tuple[int, int]]] = defaultdict(list)
    for pair, incident in edges.items():
        if len(incident) == 2:
            a, b = incident
            relative = -directions[a, pair] * directions[b, pair]
            constraints[a].append((b, relative))
            constraints[b].append((a, relative))
    signs: dict[int, int] = {}
    for root in range(len(faces)):
        if root in signs:
            continue
        signs[root] = 1
        frontier = [root]
        while frontier:
            a = frontier.pop()
            for b, relative in constraints[a]:
                desired = signs[a] * relative
                if b in signs and signs[b] != desired:
                    return False
                if b not in signs:
                    signs[b] = desired
                    frontier.append(b)
    return True


def census(n0: int, faces: list[Face]) -> dict[str, object]:
    """Classify the finite simplicial complex, including manifold prerequisites."""
    if any(v < 0 or v >= n0 for face in faces for v in face):
        message = "vertex outside complex"
        raise ValueError(message)
    edges = incidence(faces)
    boundary = [pair for pair, incident in edges.items() if len(incident) == 1]
    boundary_vertices = {v for pair in boundary for v in pair}
    boundary_graph = graph(boundary_vertices, boundary)
    links = [link_kind(v, faces) for v in range(n0)]
    manifold = all(len(incident) in (1, 2) for incident in edges.values()) and all(
        kind == ("path" if v in boundary_vertices else "cycle") for v, kind in enumerate(links)
    )
    dual = graph(range(len(faces)), (pair for incident in edges.values() for pair in combinations(incident, 2)))
    return {
        "N0": n0,
        "N1": len(edges),
        "N2": len(faces),
        "chi": n0 - len(edges) + len(faces),
        "components": components(graph(range(n0), edges)),
        "boundary_edges": len(boundary),
        "boundary_components": components(boundary_graph),
        "boundary_components_are_cycles": all(len(adjacent) == 2 for adjacent in boundary_graph.values()),
        "edge_incidence": histogram(len(incident) for incident in edges.values()),
        "dual_degree": histogram(len(adjacent) for adjacent in dual.values()),
        "vertex_links": histogram(links),
        "manifold": manifold,
        "closed": manifold and not boundary,
        "orientable": orientable(faces, edges) if manifold else None,
    }


def adjacency_census(rows: list[list[int]], expected: Graph) -> dict[str, object]:
    """Compare directed observable arcs with the simplex-derived graph."""
    n = len(expected)
    if len(rows) != n or any(v < 0 or v >= n for row in rows for v in row):
        message = "invalid adjacency labels"
        raise ValueError(message)
    arcs = {(a, b) for a, row in enumerate(rows) for b in row}
    expected_arcs = {(a, b) for a, row in expected.items() for b in row}
    return {
        "out_degree": histogram(map(len, rows)),
        "duplicate_arcs": sum(map(len, rows)) - len(arcs),
        "missing_arcs": [list(arc) for arc in sorted(expected_arcs - arcs)],
        "extra_arcs": [list(arc) for arc in sorted(arcs - expected_arcs)],
        "nonreciprocal_arcs": [list(arc) for arc in sorted(arcs) if (arc[1], arc[0]) not in arcs],
        "weak_components": components(graph(range(n), arcs)),
    }


def audit(path: Path) -> dict[str, object]:
    """Census one snapshot and compare independent edges with all helper lists."""
    raw = read_export(path)
    observed = cast("Observation", json.loads(path.with_suffix(".json").read_text(encoding="utf-8")))
    edges = incidence(raw.faces)
    dual = graph(range(len(raw.faces)), (pair for incident in edges.values() for pair in combinations(incident, 2)))
    pointer_errors: list[list[int]] = []
    for index, ((a, b, c), neighbors) in enumerate(zip(raw.faces, raw.neighbors, strict=True)):
        for slot, pair in enumerate((edge(a, c), edge(b, c), edge(a, b))):
            if set(edges[pair]) != {index, neighbors[slot]} or len(edges[pair]) != 2:
                pointer_errors.append([index, slot])
    reconstructed_links = [edge(a, b) for a, b in observed["links"]]
    periods = max(raw.times) + 1
    bad_time_edges = [list(pair) for pair in edges if (raw.times[pair[0]] - raw.times[pair[1]]) % periods not in (0, 1, periods - 1)]
    return {
        "complex": census(len(raw.times), raw.faces),
        "sphere": observed["sphere"],
        "move_counts_reject_add_delete_flip": observed["move_counts_reject_add_delete_flip"],
        "slice_sizes": observed["slice_sizes"],
        "nonadjacent_time_edges": bad_time_edges,
        "stored_pointer_errors": pointer_errors,
        "reconstructed_links_match": Counter(reconstructed_links) == Counter(dict.fromkeys(edges, 1)),
        "observable_vertices": adjacency_census(observed["vertex_neighbors"], graph(range(len(raw.times)), edges)),
        "observable_triangles": adjacency_census(observed["triangle_neighbors"], dual),
    }


def main() -> None:
    """Recompute a report from retained exports without running another chain."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    paths = sorted(args.directory.glob("*-initial.dat")) + sorted(args.directory.glob("*-evolved.dat"))
    if not paths:
        message = "no audit snapshots found"
        raise ValueError(message)
    results = {path.stem: audit(path) for path in paths}
    args.output.write_text(json.dumps(results, indent=2, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
