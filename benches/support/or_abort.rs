//! Shared fail-fast helpers for benchmark fixture setup.

use std::fmt::Display;

/// Converts successful benchmark setup values or aborts with operation context.
pub trait OrAbort<T> {
    /// Returns the setup value or panics with the operation and failure detail.
    #[track_caller]
    fn or_abort(self, operation: impl Display) -> T;
}

impl<T, E: Display> OrAbort<T> for Result<T, E> {
    #[track_caller]
    fn or_abort(self, operation: impl Display) -> T {
        match self {
            Ok(value) => value,
            Err(error) => panic!("{operation}: {error}"),
        }
    }
}

impl<T> OrAbort<T> for Option<T> {
    #[track_caller]
    fn or_abort(self, operation: impl Display) -> T {
        let Some(value) = self else {
            panic!("{operation}");
        };
        value
    }
}
