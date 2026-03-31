# Project Structure

```
src/
├── lib.rs             # Public API and module exports
├── main.rs            # CLI entry point
├── errors.rs          # Error types (CdtError, CausalityViolation)
├── util.rs            # Safe numeric conversions, random float
├── config.rs          # Simulation configuration
├── geometry/          # Geometry abstraction layer
│   ├── traits.rs      # Core geometry traits (GeometryBackend, etc.)
│   ├── mesh.rs        # CDT-agnostic mesh data structures
│   ├── operations.rs  # High-level triangulation operations
│   ├── generators.rs  # Delaunay triangulation generators (delaunay crate boundary)
│   └── backends/      # Pluggable geometry backends
│       ├── delaunay.rs # Delaunay crate wrapper (delaunay crate boundary)
│       └── mock.rs    # Mock backend for testing
└── cdt/               # CDT physics and Monte Carlo logic
    ├── triangulation.rs # CdtTriangulation core type, factory constructors, foliation queries
    ├── foliation.rs     # Foliation struct, EdgeType enum, per-vertex time labels
    ├── action.rs        # Regge action calculation
    ├── metropolis.rs    # Metropolis-Hastings algorithm (uses markov-chain-monte-carlo crate)
    └── ergodic_moves.rs # Ergodic moves (2,2), (1,3), (3,1)
```

## Key Modules

### `cdt/foliation.rs` — Foliation

Assigns each vertex to a discrete time slice, enabling classification of edges as spacelike or timelike and triangles as up or down. See `docs/foliation.md` for design details.

- `Foliation` — aggregate bookkeeping (per-slice vertex counts, total slices)
- `EdgeType` — `Spacelike` (same slice) or `Timelike` (adjacent slices)
- `CellType` — `Up` (2,1) or `Down` (1,2) triangle classification, encoded as `i32` cell data
- Time labels are stored directly as vertex data (`Vertex.data: Option<u32>`), mirroring CDT-plusplus’s `vertex->info()`

### `cdt/triangulation.rs` — Foliation integration

- `from_foliated_cylinder(vertices_per_slice, num_slices, seed)` _(crate-internal, provisional)_ — point-set strip constructor used for internal diagnostics while explicit strip construction lands
- `assign_foliation_by_y(num_slices)` — bin existing vertices into time slices
- Query methods: `time_label`, `edge_type`, `vertices_at_time`, `slice_sizes`, `has_foliation`
- Validation: `validate_foliation()` (structural), `validate_causality()` (no edge spans >1 slice)

### `geometry/generators.rs` — Delaunay triangulation generators

- `delaunay2_with_context` — builds a 2D Delaunay triangulation with optional seed
- `build_delaunay2_with_data` — builds from coordinate + vertex-data pairs
- `random_delaunay2`, `seeded_delaunay2` — convenience wrappers
- `DelaunayTriangulation2D` — type alias for the concrete 2D triangulation type

Together with `backends/delaunay.rs`, this module is the only place that directly imports from the `delaunay` crate.

### `util.rs` — Numeric helpers

- `saturating_usize_to_i32` — safe usize→i32 for Euler characteristic arithmetic
- `y_to_time_bucket` — f64→Option<u32> via round(), for time-slice assignment
- `f64_band_to_u32` — f64→u32 clamped, for y-coordinate binning

## Key Dependencies

- `delaunay` (v0.7.4) — geometry backend (Delaunay triangulations, vertex data for time labels, `set_vertex_data_by_key` for O(1) label mutation)
- `markov-chain-monte-carlo` — MCMC framework (`Chain::step_mut`, `ProposalMut`, `Target`)
- `num-traits` — `ToPrimitive` for safe float→integer conversion
