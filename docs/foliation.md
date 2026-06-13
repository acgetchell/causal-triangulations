# Foliation Design

Per-vertex time labels, edge classification, and causal validation for 1+1 CDT.

## Background

In Causal Dynamical Triangulations (Ambjørn, Jurkiewicz, Loll 2001), spacetime is built from simplices arranged in a **foliation** — a layered structure
where each time slice is a spatial manifold and adjacent slices are connected by timelike edges.

For the periodic 1+1 CDT cases:

- **Spatial topology**: S¹ (circle) — each time slice is a ring of spacelike edges
- **Time direction**: [0, T] (cylinder) or S¹ (torus, periodic time)
- **Edge classification**: spacelike (both endpoints at same t) or timelike (endpoints at t and t±1)
- **Causality constraint**: no edge may span more than one time slice (|Δt| ≤ 1)

This implementation also supports open-boundary strip variants. `from_toroidal_cdt()` builds the periodic S¹ × S¹ toroidal case, while `from_cdt_strip()`
builds regular open spatial-interval strip geometries over discrete time. Profile constructors, `from_cdt_strip_profile()` and `from_toroidal_cdt_profile()`,
accept explicit per-slice spatial volumes `N(t)` for nonuniform initial data. All constructor families use the same edge classification and causality
constraint, but their topology metadata and boundary expectations differ.

## Architecture

Foliation is CDT domain logic. The implementation stores labels in the Delaunay-backed geometry through crate-owned wrapper APIs, but direct interaction with
upstream `delaunay::` types remains confined to the `src/geometry/` backend interface layer.

Time labels are stored **directly as vertex data** in the Delaunay triangulation, using the `Vertex<f64, u32, 2>` type parameter. This mirrors CGAL's
`vertex->info()` used in CDT-plusplus. The `Foliation` struct tracks only aggregate bookkeeping.

```text
CdtTriangulation<B>
├── geometry: B              (DelaunayBackend — owns the triangulation)
│   └── Vertex.data: Option<u32>  (per-vertex time-slice label)
├── metadata: CdtMetadata    (time_slices, dimension, history)
└── foliation: Option<Foliation>
    ├── slice_sizes: Vec<usize>  (per-slice vertex counts)
    └── num_slices: u32
```

Vertex data is set at construction time via `VertexBuilder::data(t)`. For post-construction labeling (e.g., `assign_foliation_by_y`), labels are written
in-place through CDT-owned helper paths — an O(1) operation per vertex that does not affect geometry or topology. Public callers do not receive mutable
backend access; CDT mutation paths are narrow so cache and foliation synchronization state are invalidated consistently.

## Time Label Assignment

For `from_cdt_strip()` and `from_toroidal_cdt()`, time labels are assigned directly while building vertices. Vertex `(i, t)` receives label `t`, so each slice
starts with exactly `vertices_per_slice` vertices. The profile constructors instead derive slice counts from the supplied `N(t)` vector, still storing each
vertex's label as `t`. Uniform slices are therefore a regular initial condition, not a CDT requirement.

`assign_foliation_by_y()` uses band-based bucketing and writes labels through the same CDT-owned label-write path.

## Delaunay Construction

The regular open-boundary strip constructor places vertices in a lightly perturbed layered grid with:

- **Spatial extent**: 1.0, with `vertices_per_slice` evenly spaced vertices per slice
- **Temporal gap**: 1.0, with integer y-coordinates `0, 1, 2, ...`
- **Connectivity**: produced by Delaunay point insertion, then checked for strict Up/Down simplex classification

Parameters: `vertices_per_slice ≥ 4`, `num_slices ≥ 2`.

`from_filtered_delaunay_strip()` is the CDT-plusplus-influenced construction path. It starts from an overcomplete labeled Delaunay triangulation, counts
non-strict causal simplices, removes a vertex incident to an offending simplex through the backend `remove_vertex` operation, rebuilds foliation bookkeeping
from live vertex labels, and repeats until the count converges to zero. The current pass budget is bounded, but success is defined by the CDT invariant, not by
the number of cleanup passes used.

`from_cdt_strip_profile()` places each open spatial slice according to the corresponding profile entry and delegates triangulation to the Delaunay point-data
constructor. The returned mesh must pass the same initial-constructor contract as the regular open strip: upstream Delaunay Level 1-4 validation plus CDT
topology, foliation, causality, and strict Up/Down simplex classification.

The toroidal constructor places vertices on a unit lattice in an `N × T` periodic domain and uses the upstream periodic image-point Delaunay constructor. It
then checks the requested `V = N·T`, `E = 3·N·T`, `F = 2·N·T` toroidal counts and strict CDT classification.

`from_toroidal_cdt_profile()` places each closed S¹ slice according to the corresponding profile entry and uses the periodic image-point constructor directly.
It preserves closed spatial slices, periodic time, χ = 0, and strict CDT simplex classification for the returned initial torus.

## Initialization vs Evolution Validation

Initial CDT constructors are stricter than post-move validation:

- **Initialization**: regular `from_cdt_strip()`, profiled `from_cdt_strip_profile()`, regular `from_toroidal_cdt()`, and
  `from_toroidal_cdt_profile()` must pass upstream Delaunay Level 1-4 validation before returning. This certifies those starting meshes as valid,
  well-behaved PL-manifold Delaunay triangulations.
- **After ergodic moves / simulation completion**: `CdtTriangulation::validate()` requires upstream structural validity plus CDT topology, foliation, causality,
  and simplex-classification invariants. It intentionally does not require Level 4 Delaunay-ness, because the CDT move kernels are not expected to preserve the
  Delaunay empty-circumsphere predicate.

If the move set is ever changed to preserve Delaunay-ness, final-state validation should be tightened to include Level 4 as well.

## Edge Classification

`EdgeType` is an enum:

- `Spacelike` — both endpoints share the same time slice
- `Timelike` — endpoints are in adjacent time slices (|Δt| = 1)

Classification is done by `classify_edge(t0, t1)`, which reads time labels from vertex data via `vertex_time_label()`.

## Simplex (Triangle) Classification

`SimplexType` classifies triangles by how their vertices are distributed across adjacent time slices:

- `Up` (2,1) — two vertices at time _t_, one at _t + 1_. The spacelike base is in the lower slice.
- `Down` (1,2) — one vertex at time _t_, two at _t + 1_. The spacelike base is in the upper slice.

Classification is done by `classify_simplex(t0, t1, t2)`. Triangles that don’t span exactly one time slice (e.g., all vertices at the same time, or spanning
>1 slice) return `None`.

Simplex types are encoded as `i32` simplex data (`Up = 1`, `Down = -1`) and can be bulk-written via `classify_all_simplices()` using `set_simplex_data`. For
foliated triangulations this bulk path is strict: every face must classify as `Up` or `Down`, otherwise `classify_all_simplices()` and
`validate_simplex_classification()` return a validation error.

## Strict Causal Simplex Invariant

For a current foliated triangulation, every top-dimensional simplex must be strictly causal. In the current 1+1 implementation, that means every finite
triangle face must classify as `Up` `(2,1)` or `Down` `(1,2)`. Pure spacelike faces, all-timelike/non-spacelike faces, faces spanning more than adjacent time
slices, malformed faces, and faces with missing time labels are all violations.

This is a CDT-domain invariant. It is deliberately separate from upstream Delaunay Level 4 validation: Delaunay-ness asks whether the geometric triangulation
satisfies an empty-circumsphere predicate, while strict causal simplex validation asks whether the foliation makes every top-dimensional cell an allowed CDT
cell. A useful mental model is a parallel CDT validation level rather than Delaunay Level 5:

- initial Delaunay-backed constructors require upstream Delaunay Level 4 and zero strict causal simplex violations;
- evolved CDT states must remain structurally valid and keep zero strict causal simplex violations, but they are not required to preserve the Delaunay
  empty-circumsphere property.

`strict_causal_simplex_violation_count()` exposes the invariant as a count. Valid constructor output and valid post-move states must have count zero. The
filtered constructor uses the count as its convergence condition, following the CDT-plusplus practice of repeatedly removing acausal or unclassified
simplices/vertices until no violations remain. Unlike `validate_simplex_classification()`, this counter requires current foliation bookkeeping so that zero
means the strict causal invariant was actually evaluated.

When higher-dimensional CDT support is added, this invariant should remain dimension-neutral: every top-dimensional simplex must realize one of the allowed
causal CDT simplex types for that dimension, such as the corresponding adjacent-slice distributions in 2+1, 3+1, and 4+1 dimensions.

## Validation

Two validation methods enforce foliation correctness:

### `validate_foliation()`

Structural checks:

1. Every vertex has a time label (labeled count = vertex count)
2. Every time slice is non-empty
3. `slice_sizes` sum is consistent with labeled count
4. For toroidal topology, every spatial slice is one closed S¹ ring: each vertex has exactly two spacelike neighbors in its slice, and the slice subgraph is a
   single connected cycle
5. For toroidal topology, timelike edges connect each slice to both neighboring time slices modulo `T`

Ergodic moves resynchronize foliation bookkeeping from live vertex labels after mutation. On toroidal triangulations, the move kernel immediately reruns
`validate_topology()` and `validate_foliation()` before recording success; failures are rolled back and returned as ordinary local-site rejections.

### `validate_causality_delaunay()`

Face-level check reading time labels directly from vertex data:

- Every triangle must contain exactly one spacelike edge and two timelike edges
- Returns `CdtError::CausalityViolation { time_0, time_1 }` if any triangle spans >1 slice
- Returns `CdtError::ValidationFailed { check: "causality", .. }` if a triangle is not a strict CDT simplex

### `validate_simplex_classification()`

Strict simplex-classification check:

- Succeeds vacuously when no foliation is present
- Requires every foliated face to classify as `Up` or `Down`
- Returns `CdtError::ValidationFailed { check: "simplex_classification", .. }` for same-slice or otherwise unclassifiable triangles

### `strict_causal_simplex_violation_count()`

Invariant counter:

- Requires stored foliation bookkeeping, so unfoliated triangulations return `FoliationError::MissingBookkeeping`
- Requires current foliation bookkeeping, so stale label snapshots return `FoliationError::StaleBookkeeping`
- Returns the number of finite faces that do not satisfy the strict causal simplex invariant

## Error Handling

- `CdtError::CausalityViolation { time_0, time_1 }` — structured error for time labels spanning more than one slice step
- `CdtError::DelaunayGenerationFailed` — from explicit CDT constructors when builder output is inconsistent, with detailed construction context
- `CdtError::ValidationFailed { check, detail }` — for structural foliation issues and foliation-assignment failures, for example unreadable vertex
  coordinates
- `CdtError::InvalidGenerationParameters` — for invalid constructor parameters

## Regression Coverage

- `tests/integration_tests.rs::test_toroidal_metropolis_accepts_periodic_moves_and_preserves_topology` runs a seeded toroidal Metropolis simulation, requires at
  least one accepted periodic move, and checks final topology, foliation, causality, simplex classification, and χ = 0.
