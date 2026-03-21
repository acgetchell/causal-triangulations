# Test Coverage Analysis and Improvement Suggestions

## Current Test Coverage Assessment

### Existing Tests

- **Unit tests** (`src/**`): 158 tests spread across all source modules
- **Integration tests** (`tests/integration_tests.rs`): 8 tests covering simulation workflows, topology, caching, error handling
- **CLI tests** (`tests/cli.rs`): 10 tests for command-line interface and validation
- **Python tests** (`scripts/tests/`): 455 tests for benchmark, changelog, hardware, and subprocess utilities

### Coverage Gaps Identified

## 1. **Critical: Missing Ergodic Moves Tests**

**Issue**: Ergodic moves (2,2), (1,3), (3,1), edge flips are placeholders and not properly tested with actual Tds operations.

**Recommendations**:

- Add tests for actual edge flips on Delaunay triangulations
- Test move validation (when moves should be rejected)
- Test that moves preserve Delaunay property
- Test causality constraint checks
- Add integration tests for move sequences

**File**: [`src/cdt/ergodic_moves.rs`](src/cdt/ergodic_moves.rs:172)

## 2. **Geometry Trait Operations Not Tested**

**Missing tests for**:

- [`insert_vertex()`](src/geometry/traits.rs:186) - vertex insertion
- [`remove_vertex()`](src/geometry/traits.rs:195) - vertex removal
- [`move_vertex()`](src/geometry/traits.rs:204) - vertex movement
- [`flip_edge()`](src/geometry/traits.rs:215) - edge flipping
- [`subdivide_face()`](src/geometry/traits.rs:228) - face subdivision

**File**: [`src/geometry/backends/delaunay.rs`](src/geometry/backends/delaunay.rs:363-430)

## 3. **Error Handling Coverage Gaps**

**Missing error tests**:

- Invalid parameter ranges (negative values, overflow)
- Delaunay generation failures with specific error contexts
- Action calculation errors with edge cases
- Concurrent modification scenarios

**File**: [`src/errors.rs`](src/errors.rs:1)

## 4. **Metropolis Algorithm Edge Cases**

**Missing tests for**:

- Temperature extremes (T → 0, T → ∞)
- Zero-step simulations
- Different thermalization ratios
- Measurement frequency edge cases
- Action calculation with extreme values

**File**: [`src/cdt/metropolis.rs`](src/cdt/metropolis.rs:175)

## 5. **CdtTriangulation Validation Not Tested**

**Missing tests for**:

- [`validate_cdt_properties()`](src/cdt/triangulation.rs:183)
- [`validate_topology()`](src/cdt/triangulation.rs:212)
- [`validate_causality()`](src/cdt/triangulation.rs:247) - currently placeholder
- [`validate_foliation()`](src/cdt/triangulation.rs:275) - currently placeholder
- Simulation history tracking
- Cache invalidation edge cases

**File**: [`src/cdt/triangulation.rs`](src/cdt/triangulation.rs:88)

## 6. **Mock Backend Under-tested**

**Missing tests for**:

- [`adjacent_faces()`](src/geometry/backends/mock.rs:166) - returns empty
- [`incident_edges()`](src/geometry/backends/mock.rs:174) - returns empty
- [`face_neighbors()`](src/geometry/backends/mock.rs:182) - returns empty
- Full mutation operations testing

**File**: [`src/geometry/backends/mock.rs`](src/geometry/backends/mock.rs:11)

## 7. **Geometry Operations Module**

**Missing tests for**:

- All operations in [`src/geometry/operations.rs`](src/geometry/operations.rs:1)
- No unit tests exist for this module

## 8. **Property-Based Tests Missing**

**Recommendations**:

- Add property-based tests using `proptest` or `quickcheck`
- Test invariants: Euler characteristic, Delaunay property preservation
- Test move reversibility where applicable
- Test action calculation properties

## 9. **Performance/Stress Tests**

**Missing**:

- Large triangulation performance (>1000 vertices)
- Memory usage under load
- Edge counting performance with large meshes
- Long simulation runs

## 10. **Documentation Tests**

**Missing**:

- Doc examples in public API functions
- Only one `#[doc]` example exists (marked `no_run`)

## Priority Recommendations

### HIGH PRIORITY

1. **Implement actual ergodic moves** and test with Tds
2. **Test all TriangulationMut operations** (currently stubs returning errors)
3. **Add validation tests** for CDT properties
4. **Test error propagation** end-to-end

### MEDIUM PRIORITY

1. Add property-based tests for invariants
2. Test edge cases in Metropolis algorithm
3. Complete mock backend implementation tests
4. Add geometry operations tests

### LOW PRIORITY

1. Add performance/stress tests
2. Add comprehensive doc examples

## Suggested Test Files to Create

1. `tests/ergodic_moves_integration.rs` - Full ergodic moves testing
2. `tests/geometry_operations.rs` - Geometry trait operation tests
3. `tests/property_tests.rs` - Property-based testing
4. `tests/validation_tests.rs` - CDT validation tests
5. `tests/performance_tests.rs` - Benchmark and stress tests

## Code Coverage Goals

- **Target**: 80%+ line coverage
- **Critical paths**: 95%+ coverage (action calculation, Metropolis core)
- **Error paths**: 70%+ coverage
- **Edge cases**: Document untestable scenarios

## GitHub Actions Validation

To lint all workflow files before pushing changes, run:

```bash
just action-lint
```

This command requires the [`actionlint`](https://github.com/rhysd/actionlint) binary (available via Homebrew) and validates every workflow in `.github/workflows/`.

## Next Steps

1. Run `cargo tarpaulin --out Json` to generate `tarpaulin-report.json`, then use `just coverage-report` for a per-file coverage summary. For an HTML report, run `just coverage` instead (output: `target/tarpaulin/tarpaulin-report.html`).
2. Prioritize testing unimplemented ergodic move operations
3. Add integration tests for complete simulation workflows with moves
4. Set up CI to track coverage trends over time
