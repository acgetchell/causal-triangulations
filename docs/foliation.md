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

The foliation is stored in the CDT layer (`src/cdt/foliation.rs`), not the geometry backend, preserving the CDT ↔ geometry separation.

```
CdtTriangulation<B>
├── geometry: B              (DelaunayBackend — owns the triangulation)
├── metadata: CdtMetadata    (time_slices, dimension, history)
└── foliation: Option<Foliation>
    ├── time_labels: VertexSecondaryMap<u32>   (vertex key → time slice)
    ├── slice_sizes: Vec<usize>               (per-slice vertex counts)
    └── num_slices: u32
```

`VertexSecondaryMap<u32>` is a `SparseSecondaryMap<VertexKey, u32>` from the `delaunay` crate — O(1) lookup sharing the slotmap key space with the primary vertex storage.

## Time Label Assignment

Time labels are assigned by **y-coordinate bucketing**: each vertex's y-coordinate is rounded to the nearest integer, giving the time slice index.

- Bucket for slice t: y ∈ [t − 0.5, t + 0.5)
- Conversion uses `y_to_time_bucket()` from `src/util.rs` via `ToPrimitive::to_u32`
- Values are clamped to [0, num_slices − 1]

This convention is used by both:

1. `from_foliated_cylinder()` — places vertices on a grid at integer y-coordinates, then buckets them
2. `assign_foliation_by_y_coordinate()` — bins vertices of an existing triangulation into equal y-bands
3. `validate_causality()` — re-derives time labels from coordinates for the generic (backend-agnostic) check

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

Classification is done by `Foliation::classify_edge(v0, v1)`, which looks up both vertex keys in the secondary map.

## Validation

Two validation methods enforce foliation correctness:

#### `validate_foliation()`

Structural checks (backend-agnostic):

1. Every vertex has a time label (labeled count = vertex count)
2. Every time slice is non-empty
3. `slice_sizes` sum is consistent with labeled count

#### `validate_causality()`

Edge-level check (backend-agnostic, uses y-coordinate bucketing):

- Every edge must connect vertices within the same slice or adjacent slices
- Returns `CdtError::CausalityViolation { time_0, time_1 }` if any edge spans >1 slice

There is also `validate_causality_delaunay()` on the Delaunay-specific impl, which uses the stored `VertexKey`-based time labels instead of re-deriving from coordinates.

## Error Handling

- `CdtError::CausalityViolation { time_0, time_1 }` — structured error for edges violating causality
- `CdtError::ValidationFailed { check: "foliation", detail }` — for structural foliation issues
- `CdtError::InvalidGenerationParameters` — for invalid constructor parameters
- `CdtError::InvalidParameters` — for precondition failures (e.g. no 2D coordinates available)

## Future Work

- **Toroidal topology** (S¹ × S¹): requires periodic Delaunay construction (issue #61)
- **Manual foliation assignment**: allow users to set per-vertex time labels directly
- **Foliation-aware ergodic moves**: moves that preserve or update the foliation during MCMC steps
