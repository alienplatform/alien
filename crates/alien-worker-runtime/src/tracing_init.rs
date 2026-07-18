use crate::{otlp::init_otlp_logging, Result};
use alien_error::Context;
use std::sync::{Mutex, Once};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

static TRACING_INIT: Once = Once::new();
static INIT_RESULT: Mutex<Option<Result<()>>> = Mutex::new(None);

/// Initialize tracing with optional OTLP support
/// This function sets up both local logging (JSON format) and OTLP logging if configured
/// Uses a Once guard to ensure initialization only happens once per process
pub fn init_tracing() -> Result<()> {
    TRACING_INIT.call_once(|| {
        let result = (|| -> Result<()> {
            let fmt_layer = fmt::layer()
                .json()
                .with_ansi(false)
                .with_target(false)
                .with_current_span(false)
                .with_thread_ids(true)
                .with_thread_names(true);

            let env_filter =
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

            // Initialize OTLP logging if configured
            let otlp_layer = init_otlp_logging().context(crate::error::ErrorData::Other {
                message: "Failed to initialize OTLP logging".to_string(),
            })?;

            // Set up the subscriber with conditional OTLP layer
            let registry = tracing_subscriber::registry()
                .with(fmt_layer)
                .with(env_filter);

            match otlp_layer {
                Some(otlp_layer) => {
                    registry.with(otlp_layer).init();
                }
                None => {
                    registry.init();
                }
            }

            Ok(())
        })();

        *INIT_RESULT.lock().unwrap() = Some(result);
    });

    // Return the result from the first initialization
    INIT_RESULT.lock().unwrap().as_ref().unwrap().clone()
}
