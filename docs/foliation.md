# Foliation Design

Per-vertex time labels, edge classification, and causal validation for 1+1 CDT.

## Background

In Causal Dynamical Triangulations (Ambjørn, Jurkiewicz, Loll 2001), spacetime is built from simplices arranged in a **foliation** — a layered structure where each time slice is a spatial manifold and adjacent slices are connected by timelike edges.

For the periodic 1+1 CDT cases:

- **Spatial topology**: S¹ (circle) — each time slice is a ring of spacelike edges
- **Time direction**: [0, T] (cylinder) or S¹ (torus, periodic time)
- **Edge classification**: spacelike (both endpoints at same t) or timelike (endpoints at t and t±1)
- **Causality constraint**: no edge may span more than one time slice (|Δt| ≤ 1)

This implementation also supports open-boundary strip variants. `from_toroidal_cdt()` builds the periodic S¹ × S¹ toroidal case, while `from_cdt_strip()` builds open spatial-interval strip geometries over discrete time. Both constructor families use the same edge classification and causality constraint, but their topology metadata and boundary expectations differ.

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

Vertex data is set at construction time via `VertexBuilder::data(t)`. For post-construction labeling (e.g., `assign_foliation_by_y`), labels are written in-place through CDT-owned helper paths — an O(1) operation per vertex that does not affect geometry or topology. Public callers do not receive mutable backend access; CDT mutation paths are narrow so cache and foliation synchronization state are invalidated consistently.

## Time Label Assignment

For `from_cdt_strip()` and `from_toroidal_cdt()`, time labels are assigned directly while building vertices. Vertex `(i, t)` receives label `t`, so each slice starts with exactly `vertices_per_slice` vertices and every constructed triangle spans adjacent slices.

`assign_foliation_by_y()` uses band-based bucketing and writes labels through the same CDT-owned label-write path.

## Grid Construction (`from_cdt_strip`)

The open-boundary strip constructor places vertices on a grid with:

- **Spatial extent**: 1.0, with `vertices_per_slice` evenly spaced vertices per slice
- **Temporal gap**: 1.0, with integer y-coordinates `0, 1, 2, ...`
- **Connectivity**: each quad between adjacent slices is split into one Up `(2,1)` and one Down `(1,2)` triangle

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

Cell types are encoded as `i32` cell data (`Up = 1`, `Down = -1`) and can be bulk-written via `classify_all_cells()` using `set_cell_data`. For foliated triangulations this bulk path is strict: every face must classify as `Up` or `Down`, otherwise `classify_all_cells()` and `validate_cell_classification()` return a validation error.

## Validation

Two validation methods enforce foliation correctness:

### `validate_foliation()`

Structural checks:

1. Every vertex has a time label (labeled count = vertex count)
2. Every time slice is non-empty
3. `slice_sizes` sum is consistent with labeled count

### `validate_causality_delaunay()`

Face-level check reading time labels directly from vertex data:

- Every triangle must contain exactly one spacelike edge and two timelike edges
- Returns `CdtError::CausalityViolation { time_0, time_1 }` if any triangle spans >1 slice
- Returns `CdtError::ValidationFailed { check: "causality", .. }` if a triangle is not a strict CDT cell

### `validate_cell_classification()`

Strict cell-classification check:

- Succeeds vacuously when no foliation is present
- Requires every foliated face to classify as `Up` or `Down`
- Returns `CdtError::ValidationFailed { check: "cell_classification", .. }` for same-slice or otherwise unclassifiable triangles

## Error Handling

- `CdtError::CausalityViolation { time_0, time_1 }` — structured error for time labels spanning more than one slice step
- `CdtError::DelaunayGenerationFailed` — from explicit CDT constructors when builder output is inconsistent, with detailed construction context
- `CdtError::ValidationFailed { check, detail }` — for structural foliation issues and foliation-assignment failures (for example unreadable vertex coordinates)
- `CdtError::InvalidGenerationParameters` — for invalid constructor parameters

## Future Work

- **Foliation-aware ergodic moves**: continue broadening topology-preservation tests for accepted move sequences
