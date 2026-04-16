pub mod config;
pub mod error;
pub mod events;
pub mod otlp;
pub mod runtime;
pub mod secrets;
pub mod tracing_init;
pub mod traits;
pub mod transports;

// Re-export core types
pub use error::{Error, Result};
pub use traits::{Request, Response};

// Re-export runtime
pub use runtime::{
    get_control_server, get_wait_until_server, run, setup_shutdown_on_signals, BindingsSource,
};

// Re-export config types
pub use config::{
    AppLogLine, Cli, CommandsPollingConfig, LambdaMode, LogExporter, RuntimeConfig, TransportType,
};

// Re-export event parsing modules
pub use events::*;

// Re-export OTLP functionality
pub use otlp::{flush_otlp_logs, init_otlp_logging, shutdown_otlp_logs};
pub use tracing_init::init_tracing;

pub use otlp::emit_log;
