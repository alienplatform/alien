//! Unified TUI Application
//!
//! A single app that works for both dev and platform modes.
//! The TUI is purely a view layer - all data comes from API.

pub mod config;
pub mod controller;
pub mod runtime;
pub mod state;

pub use config::{AppConfig, AppMode};
pub use controller::AppController;
pub use runtime::run_app;
pub use state::AppViewState;
