# Project Structure

```
src/
├── lib.rs             # Public API and module exports
├── main.rs            # CLI entry point
├── errors.rs          # Error types (CdtError, CausalityViolation)
├── util.rs            # Delaunay generators, safe numeric conversions
├── config.rs          # Simulation configuration
├── geometry/          # Geometry abstraction layer
│   ├── traits.rs      # Core geometry traits (GeometryBackend, etc.)
│   ├── mesh.rs        # CDT-agnostic mesh data structures
│   ├── operations.rs  # High-level triangulation operations
│   └── backends/      # Pluggable geometry backends
│       ├── delaunay.rs # Delaunay crate wrapper
│       └── mock.rs    # Mock backend for testing
└── cdt/               # CDT physics and Monte Carlo logic
    ├── triangulation.rs # CdtTriangulation core type, factory constructors, foliation queries
    ├── foliation.rs     # Foliation struct, EdgeType enum, per-vertex time labels
    ├── action.rs        # Regge action calculation
    ├── metropolis.rs    # Metropolis-Hastings algorithm (uses markov-chain-monte-carlo crate)
    └── ergodic_moves.rs # Ergodic moves (2,2), (1,3), (3,1)
```

## Key Modules

#### `cdt/foliation.rs` — Foliation

Assigns each vertex to a discrete time slice, enabling classification of edges as spacelike or timelike. See `docs/foliation.md` for design details.

- `Foliation` — aggregate bookkeeping (per-slice vertex counts, total slices)
- `EdgeType` — `Spacelike` (same slice) or `Timelike` (adjacent slices)
- Time labels are stored directly as vertex data (`Vertex.data: Option<u32>`), mirroring CDT-plusplus's `vertex->info()`

#### `cdt/triangulation.rs` — Foliation integration

- `from_foliated_cylinder(vertices_per_slice, num_slices, seed)` — grid-based CDT construction with y-coordinate bucket labeling
- `assign_foliation_by_y_coordinate(num_slices)` — bin existing vertices into time slices
- Query methods: `time_label`, `edge_type`, `vertices_at_time`, `slice_sizes`, `has_foliation`
- Validation: `validate_foliation()` (structural), `validate_causality()` (no edge spans >1 slice)

#### `util.rs` — Numeric helpers

- `saturating_usize_to_i32` — safe usize→i32 for Euler characteristic arithmetic
- `y_to_time_bucket` — f64→Option<u32> via round(), for time-slice assignment
- `f64_band_to_u32` — f64→u32 clamped, for y-coordinate binning

## Key Dependencies

- `delaunay` (v0.7.3) — geometry backend (Delaunay triangulations, vertex data for time labels)
- `markov-chain-monte-carlo` — MCMC framework (`Chain::step_mut`, `ProposalMut`, `Target`)
- `num-traits` — `ToPrimitive` for safe float→integer conversion
