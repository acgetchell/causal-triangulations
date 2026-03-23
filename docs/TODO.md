# TODO List for Causal Dynamical Triangulations

This document tracks all the pending improvements, features, and technical debt items identified in the codebase.

## High Priority - Core Functionality

### Ergodic Moves Implementation

- [ ] **Complete (2,2) move implementation** (`src/cdt/ergodic_moves.rs:160`)
  - Implement actual edge flip logic with Delaunay integration
  - Replace placeholder random acceptance with proper geometric validation
  - Add causality constraint checking

- [ ] **Complete (1,3) move implementation** (`src/cdt/ergodic_moves.rs:177`)
  - Implement vertex addition by triangle subdivision
  - Integrate with Delaunay triangulation maintenance
  - Validate geometric and causal constraints

- [ ] **Complete (3,1) move implementation** (`src/cdt/ergodic_moves.rs:193`)
  - Implement vertex removal by triangle merging
  - Ensure proper Delaunay property maintenance
  - Add causality checks for vertex removal

- [ ] **Complete edge flip implementation** (`src/cdt/ergodic_moves.rs:209`)
  - Implement standard Delaunay edge flip operations
  - Maintain causal structure during flips
  - Add geometric validity checks

### Metropolis Algorithm Improvements

- [ ] **Implement move reversal mechanism** (`src/cdt/metropolis.rs:296`)
  - Add capability to undo rejected moves
  - Implement proper state rollback for failed Metropolis steps
  - Consider implementing moves that only apply after acceptance

- [ ] **Integrate Tds-based ergodic moves** (`src/cdt/metropolis.rs:264`)
  - Adapt ergodic move system to work directly with Tds structures
  - Remove placeholder "not yet implemented" rejections
  - Ensure proper triangulation state management

## Medium Priority - Code Quality

### Error Handling Improvements

- [ ] **Add comprehensive error types**
  - Create custom error types for CDT-specific failures
  - Replace remaining `unwrap()` calls with proper error propagation
  - Add error handling for triangulation generation failures

- [ ] **Improve panic documentation**
  - Document all remaining panic conditions
  - Consider replacing panics with `Result` types where appropriate
  - Add recovery mechanisms for non-fatal errors

### Performance Optimizations

- [ ] **Optimize action calculations**
  - Cache frequently computed values
  - Consider pre-computing Euler characteristic relationships
  - Profile hot paths in Monte Carlo simulation loops

- [ ] **Memory usage optimization**
  - Implement memory-efficient measurement collection
  - Consider streaming or batched measurement storage for long runs
  - Optimize triangulation data structure usage

### Testing Coverage

- [x] **Add integration tests** — 8 integration tests + 10 CLI tests + 155 unit tests + 408 Python tests
  - Complete CDT simulation workflow covered
  - Edge cases in triangulation generation covered

- [ ] **Improve unit test coverage**
  - Add tests for error conditions and edge cases
  - Test ergodic move validation logic
  - Add property-based testing for geometric invariants

## Low Priority - Features & Documentation

### New Features

- [ ] **3D CDT support**
  - Extend current 2D implementation to 3D
  - Adapt action calculations for 3D Regge calculus
  - Update ergodic moves for 3D triangulations

- [ ] **Configuration validation**
  - Add validation for physics parameter ranges
  - Implement sanity checks for simulation parameters
  - Add warnings for potentially problematic configurations

- [ ] **Output formats**
  - Add support for standard mesh output formats
  - Implement visualization data export
  - Add statistical analysis output options

### Documentation

- [ ] **Algorithm documentation**
  - Add detailed mathematical background for CDT
  - Document the specific ergodic move algorithms used
  - Explain Metropolis-Hastings implementation details

- [ ] **Usage examples**
  - Add comprehensive usage examples to library documentation
  - Create tutorial documentation for new users
  - Add examples of different simulation configurations

- [ ] **API documentation**
  - Complete all missing function documentation
  - Add examples to major public API functions
  - Document expected parameter ranges and units

## Technical Debt

### Code Organization

- [ ] **Module restructuring**
  - Consider splitting large modules into smaller, focused ones
  - Improve module-level documentation and organization
  - Standardize naming conventions across modules

### Dependencies

- [ ] **Dependency audit**
  - Review and potentially reduce dependency count
  - Update to latest stable versions of key dependencies
  - Remove any unused dependencies

### Build System

- [x] **CI/CD improvements**
  - Performance regression testing via `benchmark_utils.py` + GitHub Actions
  - Automated benchmarking with Criterion.rs baselines
  - Comprehensive linting: clippy, ruff, ty, shellcheck, yamllint, actionlint, dprint, typos, taplo

## Future Research Directions

### Advanced Features

- [ ] **Quantum corrections**
  - Investigate incorporation of quantum gravity corrections
  - Research connection to asymptotic safety program
  - Explore renormalization group flow implementations

- [ ] **Alternative actions**
  - Implement other discrete gravity actions beyond Regge
  - Add support for modified gravity theories
  - Investigate higher-order correction terms

### Computational Improvements

- [ ] **Parallelization**
  - Investigate parallel Monte Carlo implementations
  - Add multi-threading support for large simulations
  - Explore GPU acceleration possibilities

---

## Notes

- Items marked with file references indicate specific locations in the codebase
- Priority levels are suggestions and may be adjusted based on project needs
- Some items may require significant research and development effort
- Regular review and updating of this list is recommended as the project evolves

Last updated: 2026-02-21
