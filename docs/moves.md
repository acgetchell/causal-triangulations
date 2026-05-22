# Ergodic Moves

Ergodic moves are the local Monte Carlo updates that allow the triangulation to explore the space of geometries. This module implements the standard ergodic
moves for 2D Causal Dynamical Triangulations (see `src/cdt/ergodic_moves.rs`).

For public examples, doctests, benchmarks, and integration tests, import the focused move API with `use causal_triangulations::prelude::moves::*;`. Combine it
with `prelude::triangulation::*` for CDT wrappers and `prelude::geometry::*` when constructing explicit Delaunay fixtures.

## Types

### `MoveType`

Enumerates the available move types:

- `Move22` — (2,2) move: flip the shared edge between two triangles, preserving vertex count; causality-aware — the CDT layer validates and rejects moves that
  break causal layering
- `Move13Add` — (1,3) move: insert a new vertex by subdividing one triangle into three; when the triangulation is foliated, the inserted vertex receives the
  time label that keeps all replacement triangles causal
- `Move31Remove` — (3,1) move: remove a degree-3 vertex by merging three triangles into one when the replacement triangle is causal and the removal does not
  empty a time slice
- `EdgeFlip` — API-compatible alias for the 2D k=2 edge flip used by `Move22`; it records separate statistics but uses the same causal prechecks

### `MoveResult`

Returned by each `attempt_*` method:

- `Success` — move was applied
- `CausalityViolation` — rejected because the move would break causal layering
- `GeometricViolation` — rejected because no geometrically valid candidate move exists
- `Rejected(CdtError)` — rejected for another reason, with details; backend mutation failures are reported as `CdtError::BackendMutationFailed` rather than
  collapsed into `GeometricViolation`
- `HardFailure(CdtError)` — the move already mutated geometry but then failed a required post-mutation synchronization step; this is distinct from
  `Rejected(CdtError)`, which reports reversible rejection reasons before an accepted mutation is finalized. See the `MoveResult` enum and
  `HardFailure(CdtError)` variant in `src/cdt/ergodic_moves.rs`.

### `MoveStatistics`

Tracks per-move-type attempt and acceptance counts. Fields: `moves_22_attempted` / `moves_22_accepted`, `moves_13_attempted` / `moves_13_accepted`,
`moves_31_attempted` / `moves_31_accepted`, `edge_flips_attempted` / `edge_flips_accepted`.

Key methods:

- `record_attempt(MoveType)` — increment the attempt counter
- `record_success(MoveType)` — increment the acceptance counter
- `acceptance_rate(MoveType) -> f64` — ratio for a single move type
- `total_acceptance_rate() -> f64` — ratio across all move types

### `ErgodicsSystem`

Owns a `MoveStatistics` instance and a thread-local RNG. Public API:

- `new()` / `Default::default()` — construct
- `select_random_move() -> MoveType` — samples uniformly from all four move types
- `attempt_22_move(&mut CdtTriangulation2D) -> MoveResult`
- `attempt_13_move(&mut CdtTriangulation2D) -> MoveResult`
- `attempt_31_move(&mut CdtTriangulation2D) -> MoveResult`
- `attempt_edge_flip(&mut CdtTriangulation2D) -> MoveResult`
- `attempt_random_move(&mut CdtTriangulation2D) -> MoveResult` — delegates to one of the above

Accepted moves mutate the triangulation through narrow CDT-owned edit operations, then rebuild CDT foliation bookkeeping from live vertex labels and refresh
simplex classifications. On toroidal triangulations, move finalization also rechecks χ = 0 and the closed-S¹ per-slice foliation invariant before recording
success. The raw mutable backend is not exposed as part of the CDT API.

## Architecture

Move validation follows a two-layer design:

- **`delaunay` crate** — pure geometric operations (`flip_k2`, `flip_k1_insert`, `flip_k1_remove`) with no physics constraints
- **Geometry backend interface layer (`src/geometry/`)** — wraps upstream Delaunay operations behind crate-owned traits, handles conversion, validation, and
  error translation between the upstream library and our internal geometry types
- **CDT domain layer (`src/cdt/`)** — chooses candidate sites, checks causality and time-slice integrity, and resynchronizes foliation metadata after accepted
  moves

Move code lives in the CDT domain layer. It may call `DelaunayBackend2D` methods and trait-backed mutation hooks, but it must not import upstream `delaunay::`
APIs directly.

Public `attempt_*` methods snapshot only after a valid local site has been selected and mutation is about to begin; ordinary geometric or causal rejections do
not clone the triangulation. If a selected mutation or required post-mutation synchronization fails, the method restores that snapshot before returning the
non-success `MoveResult`. Toroidal post-move topology or closed-ring foliation failures are treated as rollbackable local-site rejections, because the candidate
site was geometrically editable but would break the periodic CDT contract.

The Metropolis loop accepts or rejects a move type before calling these mutating kernels. If an accepted application fails at its selected site, the simulation
retries at another random site from the restored triangulation. Exhausting those retries is recorded as a rejected proposal; hard backend mutation failures
still return `CdtError::MetropolisMoveApplicationFailed`. See `docs/metropolis.md`.

## Planned Work

- [ ] Weight `select_random_move()` by available application sites per move type to remove uniform-sampling chain bias
- [ ] Weight accepted move-site retries by available local sites instead of bounded random retries
- [ ] Broaden per-kernel toroidal move-site tests around periodic boundary simplices
