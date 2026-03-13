//! TUI Views - Pure rendering functions
//!
//! Each view module provides:
//! - `render()` - pure render function taking state
//! - `handle_key()` - returns Action, doesn't execute side effects
//! - `keybinds()` - returns available keybinds for display

pub mod commands_view;
pub mod deployment_detail_view;
pub mod deployment_groups_view;
pub mod deployments_list;
pub mod logs_view;
pub mod packages_view;
pub mod releases_view;
pub mod table;
