# Ergodic Moves

Ergodic moves are the local Monte Carlo updates that allow the triangulation to explore the space of geometries. This module implements the standard ergodic
moves for 2D Causal Dynamical Triangulations (see `src/cdt/ergodic_moves.rs`).

For public examples, doctests, benchmarks, and integration tests, import the focused move API with `use causal_triangulations::prelude::moves::*;`. Combine it
with `prelude::triangulation::*` for CDT wrappers and `prelude::geometry::*` when constructing explicit Delaunay fixtures.

## Types

### `MoveType`

Enumerates the available move types:

- `Move22` — (2,2) move: flip the shared edge between two triangles, preserving vertex count; causality-aware — the CDT layer validates and rejects moves
  that break causal layering
- `Move13Add` — (1,3) move: insert a new vertex by adding local CDT volume; open-boundary and unfoliated runs subdivide one triangle into three, while
  toroidal foliated runs split a spacelike link by subdividing an adjacent face and flipping the original spacelike link away. Inserted vertices receive the
  time label needed to keep the replacement simplices causal.
- `Move31Remove` — (3,1) move: remove a vertex by collapsing local CDT volume; open-boundary and unfoliated runs remove a degree-3 vertex when the replacement
  triangle is causal and no time slice is emptied, while toroidal foliated runs use the inverse flip-then-collapse path for degree-4 spacelike-link-split
  configurations.
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

Tracks per-move-type attempts, accepted moves, and hard failures. Attempts count every selected proposal. Accepted counts include only moves that committed and
validated successfully. Hard failures are distinct from ordinary proposal rejections: they remain in the attempt denominator, do not increment accepted counts,
and are reported through the `*_hard_failed` fields and `total_hard_failures()`.

Key methods:

- `record_attempt(MoveType)` — increment the attempt counter
- `record_success(MoveType)` — increment the committed-and-validated acceptance counter
- `record_hard_failure(MoveType)` — increment post-mutation invariant failure telemetry without treating the move as accepted
- `acceptance_rate(MoveType) -> f64` — accepted / attempted ratio for a single move type
- `total_acceptance_rate() -> f64` — accepted / attempted ratio across all move types
- `total_hard_failures() -> u64` — total hard failures across all move types

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

The Metropolis loop first selects an explicit local proposal site, clones the current triangulation, and applies that exact site on the cloned proposed state;
see `src/cdt/metropolis/runner.rs` and `src/cdt/metropolis/adapter.rs`. The CDT proposal adapter scores the planned move with its action change and
forward/reverse local-site ratio, then the upstream `markov-chain-monte-carlo` sampler owns the Metropolis-Hastings accept/reject draw and generic chain
counters; see `docs/metropolis.md`. Only accepted proposals swap the cloned, mutated state into the live simulation. Ordinary causal, geometric, or backend
edit failures on the cloned state are self-loop proposal outcomes recorded in `ProposalStatistics`; hard backend mutation or invariant-refresh failures still
return `CdtError::MetropolisMoveApplicationFailed`.

## Ensemble And Volume Fixing

Current CDT simulations do not add a volume-fixing term. The `(1,3)` and `(3,1)` kernels are genuine volume-changing proposals, so accepted moves may change
the total number of vertices and simplices over time. The resulting Markov chain should therefore be interpreted as sampling the unfixed-volume ensemble
specified by the configured action, cosmological term, proposal probabilities, and Metropolis-Hastings acceptance rule.

This is a valid 1+1 CDT setting rather than an implementation accident. Israel and Lindner use a 1+1 CDT Monte Carlo model where add/remove moves satisfy
detailed balance and the cosmological constant controls whether the universe size is stable, shrinking, or diverging:
[Quantum gravity on a laptop: 1+1 Dimensional Causal Dynamical Triangulation simulation](https://doi.org/10.1016/j.rinp.2012.10.001).

For unfixed-volume runs, tune the cosmological constant rather than expecting the move kernels to preserve volume. The cosmological constant is conjugate to
the lattice volume term, so changing it changes the relative acceptance of volume-increasing and volume-decreasing trajectories through the ordinary action
difference. The current binary exposes this as `--cosmological-constant`, and programmatic callers set it through `ActionConfig`.

The default 1+1 action constants are calibrated to the standard 2D CDT critical cosmological coupling. They use zero curvature couplings and set the edge-count
cosmological constant to `(2 / 3) ln 2`, mapping this crate's `λ N1` action convention to `λc N2` with `λc = ln 2` on closed toroidal triangulations.

Approximate volume fixing is also standard in higher-dimensional CDT when the scientific goal is a finite-size ensemble at a chosen lattice volume. In that
case the fixing term is part of the sampled action, for example a quadratic penalty around a target volume, and detailed balance is with respect to the
modified ensemble. See Ambjørn et al.,
[The Semiclassical Limit of Causal Dynamical Triangulations](https://arxiv.org/abs/1102.3929), and the toroidal topology study,
[The phase structure of Causal Dynamical Triangulations with toroidal spatial topology](https://arxiv.org/abs/1802.10434). This crate does not implement volume
fixing yet; if added, it should be explicit and opt-in.

## Planned Work

- [ ] Weight `select_random_move()` by available application sites per move type to remove uniform-sampling chain bias
- [ ] Broaden per-kernel toroidal move-site tests around periodic boundary simplices
