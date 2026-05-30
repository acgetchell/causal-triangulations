# Security Policy

## Supported Versions

Use the latest released crate version or the default branch. Security fixes are not backported to older versions unless noted in a release.

This crate is pre-1.0 and under active development, so API compatibility and security support are tied to the current release line.

## Reporting a Vulnerability

Please report vulnerabilities privately using GitHub private vulnerability reporting:

<https://github.com/acgetchell/causal-triangulations/security/advisories/new>

Do not open a public issue for suspected vulnerabilities.

Include:

- Affected crate version or commit.
- Enabled Cargo features and any relevant simulation configuration.
- Steps to reproduce, ideally with a minimal Rust example or test.
- Expected and observed behavior.
- Any relevant triangulation, checkpoint, serialized input, or workflow context, with sensitive data removed.

For numerical correctness issues that are not security-sensitive, open a normal GitHub issue with a minimal reproduction.

## Security Checks

This project uses GitHub CodeQL, Dependabot security updates, secret scanning with push protection, `cargo audit`, zizmor, Clippy SARIF analysis, and
repository-owned Semgrep rules.
