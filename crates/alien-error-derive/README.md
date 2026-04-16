# alien-error-derive

Proc macro crate for alien-error. Provides `#[derive(AlienErrorData)]` which generates error metadata from `#[error(...)]` attributes on enum variants — code, message (with field interpolation), retryable, internal, HTTP status, and hint.
