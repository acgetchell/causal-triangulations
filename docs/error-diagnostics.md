# Typed Error Diagnostics

## Choosing an error to match

The structured validation payloads introduced by [#149](https://github.com/acgetchell/causal-triangulations/issues/149) distinguish the rejected input from the
constraint it failed. Import `CdtError`, `ObservedValue`, `ExpectedConstraint`, and the category enums from `prelude::errors`.

| Error variant | Category field and type | Constraint field |
| --- | --- | --- |
| `InvalidConfiguration` | `setting: ConfigurationSetting` | `expected` |
| `InvalidSimulationConfiguration` | `setting: ConfigurationSetting` | `expected` |
| `InvalidGenerationParameters` | `issue: GenerationParameterIssue` | `expected_range` |
| `InvalidTriangulationMetadata` | `field: TriangulationMetadataField` | `expected` |

All four variants carry `provided_value: ObservedValue`. Numeric payloads can preserve a profile index, the operands of an overflowing count calculation, or
the original bits of a floating-point input. Match `ExpectedConstraint` for bounds and relations such as topology-dependent minima or divisibility by the
time-slice count. Both payload enums are non-exhaustive. Use a fallback arm and inspect special floats with `is_nan`, sign checks, or `to_bits`.

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

Use `Display` for logs and CLI messages. The formatted diagnostic includes the category, observed value, and expected condition. Use typed patterns for
program decisions. A Metropolis backend failure carries its stage and a typed source error. See [ordinary rejections](metropolis.md#ordinary-rejections)
for how the sampler handles unsuccessful proposals.

## Migrating string payload matches

Replace string comparisons with nested `ObservedValue` and `ExpectedConstraint` matches. Category-only matches using `..` still work. Call `.to_string()`
on a payload when displaying it. Diagnostic values are Rust error records. See [checkpoint compatibility](metropolis.md#serialized-checkpoint-compatibility)
for the independent checkpoint wire contract.
