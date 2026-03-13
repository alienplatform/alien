//! TUI Storybook - testing pure view components
//!
//! This module provides demos for testing TUI views in isolation.
//! Views are pure render functions that take state and render it.
//! No SDK calls or network access is needed.

pub mod demos;
pub mod mock_data;
pub mod runner;
pub mod scenarios;

pub use runner::run_demo;
