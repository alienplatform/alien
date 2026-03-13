//! TUI Framework - Core components for building the unified TUI
//!
//! This module provides reusable framework components:
//! - `App` - Main application container with event loop and tab navigation
//! - `Header` - Application header with branding and connection info
//! - `Tabs` - Tab bar widget for switching between views
//! - `Table` - Searchable table component for list views
//! - `Search` - Search overlay activated by `/`
//! - `Dialog` - Modal dialog framework
//! - `Keybinds` - Footer keybind display

pub mod app;
pub mod dialog;
pub mod header;
pub mod keybinds;
pub mod search;
pub mod table;
pub mod tabs;

// Re-export from state module for backwards compat
pub use crate::tui::state::{Action, AppState, InputMode, SearchState, ViewId};

pub use header::Header;
pub use keybinds::{Keybinds, KeybindsFooter};
pub use search::SearchOverlay;
pub use table::{SearchableTable, TableCell, TableColumn, TableRow, TableState};
pub use tabs::TabBar;
