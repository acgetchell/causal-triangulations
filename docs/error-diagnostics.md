# Typed Error Diagnostics

## Proposal and scope

This public API proposal was recorded before implementation for [issue #149](https://github.com/acgetchell/causal-triangulations/issues/149), against
`90cc2e019713fceb22c8bdb6d3d3d028aeefd557`. It preserves existing validation order, accepted inputs, numerical operations, and error-layer boundaries.

Keep `ConfigurationSetting`, `GenerationParameterIssue`, and `TriangulationMetadataField` as finite category identities. A category does not contain the
value from one failed call. Keep the four enclosing errors because they distinguish configuration repair, simulation scheduling, generator input, and
inconsistent triangulation state. Their current producers justify changing all four payloads:

| Error variant | Observed information currently rendered into strings | Required constraint information |
| --- | --- | --- |
| `InvalidConfiguration` | Counts, profile lengths/slices, profile sums, and action couplings | Bounds, topology, divisibility, count relationships, finite action range |
| `InvalidSimulationConfiguration` | Temperature and measurement schedule | Positive finite reciprocal, finite target range, steps bound, viable measurement schedule |
| `InvalidGenerationParameters` | Counts, overflow operands, profiles, coordinates, and periods | Minimums, finite geometry inputs, supported count arithmetic |
| `InvalidTriangulationMetadata` | Time-slice count and dimension | Topology minimum, backend dimension, platform count capacity |

## Public shape

Retain the existing field names. Replace every `provided_value: String` in these four variants with `ObservedValue`, and every `expected: String` or
`expected_range: String` with `ExpectedConstraint`. Export both types from the crate root and `prelude::errors`; keep the broad quick-start prelude small.

`ObservedValue` is a non-exhaustive enum containing concrete numeric data, rather than a generic formatted-text escape hatch:

- `Count(u128)` preserves unsigned counts, including platform-sized lengths, without truncation.
- `Float(f64)` retains the original floating-point value, including NaN, infinities, signed zero, and subnormals.
- `CoordinateRange { min, max }`, `VertexCoordinate { vertex_index, axis, value }`, and `ToroidalPeriod { axis, value }` retain geometry context.
- `ToroidalDomain { periods, detail }` preserves both periods and an opaque upstream diagnostic on the defensive backend-conversion path.
- `SpatialVertexProfile(Vec<u32>)` and `ProfileSlice { index, vertices }` identify profile failures.
- `CountProduct { left, right }` and `CountSum { left, right }` preserve overflowing operands instead of attempting overflowing diagnostic arithmetic.
- `ActionCouplings { coupling_0, coupling_2, cosmological_constant }` and `MeasurementSchedule { steps, thermalization_steps, measurement_frequency }`
  preserve related inputs together.

`ExpectedConstraint` is a non-exhaustive enum for the actual validation contracts. Shared count constraints use `AtLeast` and `Exactly` with numeric payloads;
upper bounds use `AtMostSteps` to retain their schedule dependency. Relational constraints preserve their dependencies: topology-dependent minimums,
divisibility by time slices, minimum total vertices, profile length and sum, maximum steps, and backend dimension. Distinct variants identify finite values,
finite increasing coordinate bounds, positive periods, positive finite reciprocal, safe action magnitude, and finite log probability. The latter two retain the
maximum simplex count or action magnitude used by the existing check. Overflow constraints identify `u32`/`usize` capacity, profile-sum capacity, or checked
open-strip face-count evaluation.

Both enums implement `Debug`, `Clone`, `PartialEq`, and `Display`. They are diagnostic records, not a second validation engine: do not add a generic
`is_valid` predicate that could drift from the domain validators. Floating-point payloads deliberately do not implement `Eq`; use `is_nan`, sign checks,
or `to_bits` when inspecting special values. Public matches need a fallback for future variants.

## Matching and human-readable output

For a failed `CdtConfig::into_validated`, callers can match the specific category and constraint without parsing text:

```rust
use causal_triangulations::prelude::errors::{
    CdtError, ConfigurationSetting, ExpectedConstraint, ObservedValue,
};

fn rejected_vertex_count(error: &CdtError) -> Option<(u128, u128)> {
    match error {
        CdtError::InvalidConfiguration {
            setting: ConfigurationSetting::Vertices,
            provided_value: ObservedValue::Count(actual),
            expected: ExpectedConstraint::AtLeast { minimum },
        } => Some((*actual, *minimum)),
        _ => None,
    }
}
```

Use `Display` for logs and CLI diagnostics. It continues to report the failing category, observed value, expected condition, and relevant topology/index.
Preserve existing wording where practical, but wording is not a machine-readable schema. Domain-specific relationships may add numeric context previously
omitted from the message. Keep opaque upstream detail only at the backend boundary.

No new top-level error split is needed: current category enums and typed constraints distinguish corrective actions within each layer. Backend failures,
unsupported dimensions, and topology mismatches already have separate variants and remain separate.

## Compatibility and release notes

**Breaking change:** constructing these four public variants or reading their former string fields requires migration to `ObservedValue` and
`ExpectedConstraint`. Existing category-only matches using `..` continue to work. Replace string comparisons with nested typed patterns; use
`provided_value.to_string()` or `expected.to_string()` only when rendered output is desired.

The crate is currently 0.1.1. Under [Cargo's compatibility convention](https://doc.rust-lang.org/cargo/reference/semver.html#change-categories), this belongs
in a breaking release such as 0.2.0, not a compatible 0.1.x patch. This issue does not change package versions, create tags, or publish a release.
The implementation commit must carry a `BREAKING CHANGE` trailer and regenerate release notes through `just changelog`.

`CdtError` and its current category enums do not implement Serde. Do not introduce a diagnostic wire format here: in particular, ordinary JSON floats cannot
losslessly represent NaN and infinities. Configuration, result, geometry, and checkpoint serialization remain unchanged. Their decoding paths must continue
to preserve typed validation errors where the existing API does so; Serde's text-only error boundary still renders the same useful diagnostics.

## Validation contract

Migrate affected tests and public examples to structured payload matches, including boundary counts, profile indices and sums, overflow operands, invalid
coordinates/periods, non-finite and tiny temperatures, and action-range failures. Check useful `Display` text separately from behavior assertions.
Exercise propagation through configuration overrides, constructor helpers, and checkpoint restoration. Keep numerical predicates and check order unchanged.
Run focused error/producer tests and doctests, followed by the repository's complete `just ci` and exact-head native CI.
