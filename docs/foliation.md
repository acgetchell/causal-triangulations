# Foliation Design

Per-vertex time labels, edge classification, and causal validation for 1+1 CDT.

## Background

In Causal Dynamical Triangulations (Ambjørn, Jurkiewicz, Loll 2001), spacetime is built from simplices arranged in a **foliation** — a layered structure where each time slice is a spatial manifold and adjacent slices are connected by timelike edges.

For 1+1 CDT:

- **Spatial topology**: S¹ (circle) — each time slice is a ring of spacelike edges
- **Time direction**: [0, T] (cylinder) or S¹ (torus, periodic time)
- **Edge classification**: spacelike (both endpoints at same t) or timelike (endpoints at t and t±1)
- **Causality constraint**: no edge may span more than one time slice (|Δt| ≤ 1)

This implementation uses **cylinder topology** (S¹ × [0,T]) — spatial slices are open chains within the Delaunay triangulation, time runs from 0 to T−1. Toroidal topology (periodic time, full S¹ spatial slices) requires upstream support for periodic Delaunay triangulation (see issue #61).

## Architecture

Time labels are stored **directly as vertex data** in the Delaunay triangulation, using the `Vertex<f64, u32, 2>` type parameter. This mirrors CGAL's `vertex->info()` used in CDT-plusplus. The `Foliation` struct tracks only aggregate bookkeeping.

```text
CdtTriangulation<B>
├── geometry: B              (DelaunayBackend — owns the triangulation)
│   └── Vertex.data: Option<u32>  (per-vertex time-slice label)
├── metadata: CdtMetadata    (time_slices, dimension, history)
└── foliation: Option<Foliation>
    ├── slice_sizes: Vec<usize>  (per-slice vertex counts)
    └── num_slices: u32
```

Vertex data is set at construction time via `VertexBuilder::data(t)`. For post-construction labeling (e.g., `assign_foliation_by_y_coordinate`), labels are written in-place via `DelaunayTriangulation::set_vertex_data(key, Some(t))` — an O(1) operation per vertex that does not affect geometry or topology.

## Time Label Assignment

For `from_foliated_cylinder()`, time labels are assigned by **y-coordinate bucketing**: each vertex's y-coordinate is rounded to the nearest integer, giving the time slice index. The label is embedded directly as vertex data at construction.

- Bucket for slice t: y ∈ [t − 0.5, t + 0.5)
- Conversion uses `y_to_time_bucket()` from `src/util.rs` via `ToPrimitive::to_u32`
- Values are clamped to [0, num_slices − 1]

`assign_foliation_by_y_coordinate()` uses band-based bucketing and writes labels directly to vertex data via `set_vertex_data`.

## Grid Construction (`from_foliated_cylinder`)

The constructor places vertices on a grid with:

- **Spatial extent**: 1.0 (fixed, below the √3 ≈ 1.73 threshold that guarantees no Delaunay edge skips a time slice)
- **Temporal gap**: 1.0 (integer y-coordinates: y = 0, 1, 2, ...)
- **Perturbation**: small deterministic perturbation breaks co-circular degeneracy
  - Interior vertices: hash-based random perturbation in x and y
  - Boundary vertices (i=0, i=last): concave √(t+1) x-offset pushed outward, ensuring every row's extreme vertices are on the convex hull (no hull edge skips a time slice)

Parameters: `vertices_per_slice ≥ 4`, `num_slices ≥ 2`.

## Edge Classification

`EdgeType` is an enum:

- `Spacelike` — both endpoints share the same time slice
- `Timelike` — endpoints are in adjacent time slices (|Δt| = 1)

Classification is done by `classify_edge(t0, t1)`, which reads time labels from vertex data via `vertex_time_label()`.

## Cell (Triangle) Classification

`CellType` classifies triangles by how their vertices are distributed across adjacent time slices:

- `Up` (2,1) — two vertices at time _t_, one at _t + 1_. The spacelike base is in the lower slice.
- `Down` (1,2) — one vertex at time _t_, two at _t + 1_. The spacelike base is in the upper slice.

Classification is done by `classify_cell(t0, t1, t2)`. Triangles that don’t span exactly one time slice (e.g., all vertices at the same time, or spanning >1 slice) return `None`.

Cell types are encoded as `i32` cell data (`Up = 1`, `Down = -1`) and can be bulk-written via `classify_all_cells()` using `set_cell_data`.

## Validation

Two validation methods enforce foliation correctness:

### `validate_foliation()`

Structural checks:

1. Every vertex has a time label (labeled count = vertex count)
2. Every time slice is non-empty
3. `slice_sizes` sum is consistent with labeled count

### `validate_causality_delaunay()`

Edge-level check reading time labels directly from vertex data:

- Every edge must connect vertices within the same slice or adjacent slices
- Returns `CdtError::CausalityViolation { time_0, time_1 }` if any edge spans >1 slice

## Error Handling

- `CdtError::CausalityViolation { time_0, time_1 }` — structured error for edges violating causality
- `CdtError::ValidationFailed { check: "foliation", detail }` — for structural foliation issues
- `CdtError::InvalidGenerationParameters` — for invalid constructor parameters
- `CdtError::InvalidParameters` — for precondition failures (e.g. no 2D coordinates available)

## Future Work

- **Toroidal topology** (S¹ × S¹): requires periodic Delaunay construction (issue #61)
- **Foliation-aware ergodic moves**: moves that preserve or update the foliation during MCMC steps (#55)
