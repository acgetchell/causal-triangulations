# Performance Testing Guide

This document explains how to use the performance regression testing system for the Causal Dynamical Triangulations (CDT) library.

## Overview

The CDT project uses [Criterion.rs](https://github.com/bheisler/criterion.rs) for benchmarking with a custom performance analysis system that:

- **Detects regressions** automatically on pull requests
- **Tracks performance trends** over time
- **Generates detailed reports** with statistical analysis
- **Integrates with CI/CD** to prevent performance regressions

## Quick Start

### Running Performance Checks Locally

```bash
# Check for performance regressions (10% threshold)
just perf-check

# Check with custom threshold (5% threshold)
just perf-check 5.0

# Save current performance as baseline
just perf-baseline

# Generate detailed performance report
just perf-report

# Analyze performance trends over last 7 days
just perf-trends 7
```

### Understanding Results

The performance analysis categorizes benchmark changes:

- 🔴 **Regressions**: Performance degraded beyond threshold
- 🟢 **Improvements**: Performance improved beyond threshold
- ✅ **Stable**: Changes within acceptable range
- 🆕 **New**: First-time benchmarks

## Detailed Usage

### Command Reference

#### `just perf-baseline [tag]`

Save current benchmark results as a baseline for future comparisons.

```bash
# Save baseline with automatic timestamp
just perf-baseline

# Save baseline with version tag
just perf-baseline v1.2.0

# Save baseline for feature branch
just perf-baseline feature-optimization
```

**When to use**: Before major changes, releases, or when establishing new performance baselines.

#### `just perf-check [threshold]`

Run benchmarks and check for performance regressions.

```bash
# Default 10% regression threshold
just perf-check

# Strict 5% threshold for critical changes
just perf-check 5.0

# Relaxed 15% threshold for experimental features
just perf-check 15.0
```

**Exit codes**:

- `0`: No regressions detected
- `1`: Performance regressions found (fails CI)

#### `just perf-report [file]`

Generate detailed performance analysis report.

```bash
# Generate report with timestamp
just perf-report

# Save to specific file
just perf-report release-v1.2-performance.md

# Generate report without running benchmarks
uv run performance-analysis --no-run --report my-report.md
```

#### `just perf-trends [days]`

Analyze performance trends over time.

```bash
# Last week's trends
just perf-trends 7

# Last month's trends  
just perf-trends 30

# Analyze specific benchmarks
uv run performance-analysis --trends 14
```

### Advanced Usage

#### Comparing Specific Baselines

```bash
# Compare against specific baseline file
uv run performance-analysis --compare performance_baselines/baseline_v1.0.0_20231201_120000.json

# Skip running benchmarks, use cached results
uv run performance-analysis --no-run --threshold 5.0
```

#### Python API Usage

```python
from scripts.performance_analysis import PerformanceAnalyzer
from pathlib import Path

# Initialize analyzer
analyzer = PerformanceAnalyzer(Path("."))

# Extract current results
results = analyzer.extract_criterion_results()

# Compare with baseline
baseline = analyzer.load_baseline()
comparison = analyzer.compare_results(results, baseline, threshold=10.0)

# Generate report
report = analyzer.generate_report(comparison)
```

## CI/CD Integration

### Automatic Performance Testing

The project runs performance tests automatically:

#### On Pull Requests

- 🔍 **Regression Detection**: Compares PR performance against main branch baseline
- 📊 **Detailed Reports**: Posts performance analysis as PR comments
- ❌ **Blocks Merging**: Fails CI if regressions exceed threshold
- 📁 **Artifact Storage**: Uploads performance reports for review

#### On Main Branch

- 💾 **Baseline Updates**: Automatically saves new baselines after successful merges
- 🏷️ **Tagging**: Baselines tagged with commit SHA and timestamp
- 📈 **Trend Tracking**: Enables long-term performance monitoring

### Manual Workflow Triggers

You can manually trigger performance analysis from GitHub Actions:

1. Go to **Actions** tab in GitHub repository
2. Select **Performance Testing** workflow
3. Click **Run workflow**
4. Configure options:
   - **Threshold**: Regression detection sensitivity (default: 10%)
   - **Save Baseline**: Whether to save results as new baseline

### Interpreting CI Results

#### Successful Performance Check ✅

```text
✅ Performance Check Passed
   Total benchmarks: 40
   Regressions: 0
   Improvements: 3
   Stable: 37
   New: 0
```

#### Performance Regression Detected ❌

```text
🔴 Performance regressions detected!
   triangulation_creation/delaunay_backend/100: +15.2% slower
   Current: 12.5ms, Baseline: 10.8ms
```

The CI will:

- ❌ **Fail the workflow** to prevent merging
- 💬 **Post detailed comment** with regression analysis
- 📊 **Upload report artifact** for detailed investigation

## Benchmark Categories

### Critical Benchmarks (Strict Thresholds)

- **Triangulation Creation**: Core algorithm performance
- **Action Calculations**: Physics computation efficiency
- **Cached Operations**: Memory/caching system performance

### Standard Benchmarks

- **Geometry Queries**: Mesh interrogation operations
- **Ergodic Moves**: Monte Carlo move operations
- **Validation**: Correctness checking performance

### Variable Benchmarks (Relaxed Thresholds)

- **Metropolis Guardrails**: Current `MetropolisAlgorithm::run()` unsupported-operation path while real CDT move kernels are pending
- **File I/O Operations**: System-dependent operations

## Performance Optimization Workflow

### 1. Identify Bottlenecks

```bash
# Generate detailed performance report
just perf-report bottleneck-analysis.md

# Analyze recent trends
just perf-trends 30
```

### 2. Create Optimization Branch

```bash
git checkout -b perf/optimize-triangulation
```

### 3. Save Pre-optimization Baseline

```bash
just perf-baseline pre-optimization
```

### 4. Implement Optimizations

Make your performance improvements...

### 5. Measure Impact

```bash
# Check improvements against pre-optimization baseline
uv run performance-analysis --compare performance_baselines/baseline_pre-optimization_*.json

# Strict threshold to ensure meaningful improvement
just perf-check 3.0
```

### 6. Document Changes

Include performance results in your PR description:

```markdown
## Performance Impact

- Triangulation creation: **25% faster** (8.2ms → 6.1ms)
- Memory usage: **15% reduction** in peak allocation
- Cache hit rate: **Improved from 85% to 94%**
```

## Troubleshooting

### Common Issues

#### "No benchmark results found"

```bash
# Ensure benchmarks are compiled and run first
cargo bench
just perf-check
```

#### "No baseline found for comparison"

```bash
# Create initial baseline
just perf-baseline initial

# Or run against existing results
uv run performance-analysis --no-run
```

#### "Performance variance too high"

- Run benchmarks multiple times to establish confidence
- Check for system load during benchmark execution
- Consider using cloud CI for consistent results

### Performance Investigation

#### Detailed Timing Analysis

```bash
# Run with verbose output
RUST_BACKTRACE=1 cargo bench -- --verbose

# Analyze specific benchmark group
cargo bench triangulation_creation
```

#### Memory Profiling Integration

```bash
# Run benchmarks with memory profiling (requires additional tools)
cargo bench --features memory-profiling
```

## Best Practices

### For Contributors

1. **Run performance checks** before submitting PRs
2. **Include performance impact** in PR descriptions
3. **Investigate regressions** thoroughly - they may indicate real issues
4. **Save baselines** before making significant algorithmic changes

### For Maintainers

1. **Review performance comments** on PRs carefully
2. **Update baselines** after confirming acceptable changes
3. **Monitor long-term trends** using `just perf-trends`
4. **Set appropriate thresholds** for different types of changes

### Performance-Sensitive Development

```bash
# Before implementing new features
just perf-baseline feature-start

# During development - check impact frequently  
just perf-check 15.0  # Relaxed threshold during development

# Before finalizing - ensure no major regressions
just perf-check 5.0   # Strict threshold before completion
```

## Configuration

### Threshold Guidelines

| Change Type   | Recommended Threshold | Rationale                                   |
| ------------- | --------------------- | ------------------------------------------- |
| Bug fixes     | 5%                    | Should not impact performance significantly |
| New features  | 10-15%                | May have some performance cost              |
| Optimizations | 3%                    | Should show measurable improvement          |
| Experimental  | 20%                   | Exploratory changes, focus on functionality |

### Baseline Management

- **Main branch baselines**: Automatically saved on merge
- **Feature baselines**: Manually saved with descriptive tags
- **Release baselines**: Tagged with version numbers
- **Retention**: Last 10 baselines kept automatically

## Architecture

### Components

1. **Criterion Benchmarks** (`benches/cdt_benchmarks.rs`): Core benchmark definitions
2. **Performance Analyzer** (`scripts/performance_analysis.py`): Analysis and reporting engine
3. **Justfile Integration**: User-friendly command interface
4. **GitHub Actions** (`.github/workflows/performance.yml`): CI/CD automation
5. **Baseline Storage** (`performance_baselines/`): Historical performance data

### Data Flow

```text
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ Criterion       │───▶│ Performance      │───▶│ Reports &       │
│ Benchmarks      │    │ Analysis Script  │    │ Comparisons     │
└─────────────────┘    └──────────────────┘    └─────────────────┘
         │                       │                        │
         ▼                       ▼                        ▼
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
│ JSON Results    │    │ Baseline         │    │ CI Comments &   │
│ (target/criterion)│    │ Storage          │    │ Artifacts       │
└─────────────────┘    └──────────────────┘    └─────────────────┘
```

This system provides comprehensive performance monitoring while remaining easy to use for both contributors and maintainers.
