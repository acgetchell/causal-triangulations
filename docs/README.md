# Documentation

Start with [the project README](../README.md) to choose a workflow and run a small simulation. The
[published API reference](https://docs.rs/causal-triangulations/latest/causal_triangulations/) owns callable contracts and Rust examples.

## Contents

- [Using CDT](#using-cdt)
- [Scientific contracts](#scientific-contracts)
- [Contributing and maintaining](#contributing-and-maintaining)

## Using CDT

- [CLI runs](RUNNING.md): configuration, topologies, output formats, and troubleshooting.
- [Cluster runs](RUNNING-ON-HPC.md): Slurm and Open OnDemand workflows.
- [Quickstart notebook](../notebooks/00_quickstart.ipynb): a local run and diagnostic plots.
- [Visualization notebook](../notebooks/01_spacetime_visualization.ipynb): render an exported mesh to PNG and SVG under `target/notebooks/visualization/`.
- [Typed error diagnostics](error-diagnostics.md): structured matching, human-readable output, and migration from string payloads.
- [Analysis notebook](../notebooks/02_analysis_caches.ipynb): Polars caches and exploratory diagnostics.
- [Chain analysis](ANALYZING.md): explicit volume fixing, circular-loop artifacts, and dependence-aware diagnostics.
- [External 1+1 validation](VALIDATING.md) defines the batch runner, bundle format, and packaged consumer test.

## Scientific contracts

- [Scientific basis](scientific-basis.md): model scope, shared conventions, methods, and evidence limits.
- [Finite-volume validation](finite-volume-validation.md) reports the development and independent production comparisons, their exact reference,
  uncertainty estimates and reproduction commands.
- [Reference audits](audits/README.md): pinned external-source investigations and limits on parity claims.
- [References](../REFERENCES.md): thematic bibliography and stable citation anchors.
- [Foliation](foliation.md), [moves](moves.md), and [Metropolis sampling](metropolis.md): detailed domain contracts.
- [Test coverage](testing.md): implementation evidence and remaining gaps.
- [Roadmap](roadmap.md): planned analytic validation and higher-dimensional capabilities.

## Contributing and maintaining

- [CONTRIBUTING](../CONTRIBUTING.md): setup, validation, security, and PR preparation.
- [Development workflows](dev/DEVELOPING.md): command contracts and documentation policy.
- [Rust](dev/rust.md), [Python](dev/python.md), and [testing](dev/testing.md): development rules.
- [Code organization](code-organization.md): architecture boundaries and file inventory.
- [Tooling alignment](dev/tooling-alignment.md): shared-tool adoption and documented differences.
- [Tooling contracts](../tooling/README.md): consumer tests and the unified Python tooling environment.
- [Benchmarking](BENCHMARKING.md): measurement, retained evidence, and report publication.
- [Releasing](RELEASING.md) covers release preparation, pre-tag evidence, draft review, and registry verification.
- [Recovering a release](RECOVERING_RELEASES.md) shows how to resume an interrupted release from its original evidence and provider records.
- [Security policy](../SECURITY.md): private vulnerability reporting.

Generated changelog history remains in [the root changelog](../CHANGELOG.md) and [archives](archives/changelog/).
Future performance reports are generated under `docs/performance/v2/` from retained evidence; this index does not imply a measured comparison already exists.
