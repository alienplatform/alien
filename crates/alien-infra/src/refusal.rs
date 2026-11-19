//! Errors from an extension's hooks that the same input meets again, so a retry can't help.

use alien_core::ErrorData;
use alien_error::{AlienError, AlienErrorData, Context};
use serde::Serialize;

/// An error the same input meets again: never retried.
pub(crate) fn refusal(data: ErrorData) -> AlienError<ErrorData> {
    let mut error = AlienError::new(data);
    error.retryable = false;
    error
}

pub(crate) trait RefusalContext<T> {
    /// `.context(data)`, never retried.
    fn context_refused(self, data: ErrorData) -> Result<T, AlienError<ErrorData>>;
    /// `.context(data)`, retried only when the wrapped error is: a hook's own error that says
    /// another try may pass. A hook stopped at its deadline, or failed any other way, is not.
    fn context_as_wrapped(self, data: ErrorData) -> Result<T, AlienError<ErrorData>>;
}

impl<T, E> RefusalContext<T> for Result<T, AlienError<E>>
where
    E: AlienErrorData + Clone + std::fmt::Debug + Serialize + 'static + Send + Sync,
{
    fn context_refused(self, data: ErrorData) -> Result<T, AlienError<ErrorData>> {
        self.context(data).map_err(|mut error| {
            error.retryable = false;
            error
        })
    }

    fn context_as_wrapped(self, data: ErrorData) -> Result<T, AlienError<ErrorData>> {
        self.map_err(|error| {
            let retryable = error.retryable;
            let mut wrapped = Err::<(), _>(error).context(data).unwrap_err();
            wrapped.retryable = retryable;
            wrapped
        })
    }
}
