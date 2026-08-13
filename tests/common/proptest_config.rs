#![forbid(unsafe_code)]

use proptest::test_runner::Config as ProptestConfig;

/// Preserve a suite-specific local default while honoring `PROPTEST_CASES`.
pub fn with_default_cases(default_cases: u32) -> ProptestConfig {
    let config = ProptestConfig::default();
    if std::env::var_os("PROPTEST_CASES").is_some() {
        config
    } else {
        ProptestConfig {
            cases: default_cases,
            ..config
        }
    }
}
