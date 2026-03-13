//! Generic list state for table views
//!
//! A reusable state container for any list-based view with selection,
//! loading, and error handling.

/// Generic state for a list view
#[derive(Debug, Clone)]
pub struct ListState<T> {
    /// Items in the list
    pub items: Vec<T>,
    /// Currently selected index
    pub selected: Option<usize>,
    /// Whether data is being loaded
    pub loading: bool,
    /// Error message if loading failed
    pub error: Option<String>,
    /// Scroll offset for rendering
    pub offset: usize,
}

impl<T> Default for ListState<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ListState<T> {
    /// Create a new empty list state
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected: None,
            loading: false,
            error: None,
            offset: 0,
        }
    }

    /// Create a list state in loading mode
    pub fn loading() -> Self {
        Self {
            items: Vec::new(),
            selected: None,
            loading: true,
            error: None,
            offset: 0,
        }
    }

    /// Create a list state with an error
    pub fn with_error(message: impl Into<String>) -> Self {
        Self {
            items: Vec::new(),
            selected: None,
            loading: false,
            error: Some(message.into()),
            offset: 0,
        }
    }

    /// Create a list state with items
    pub fn with_items(items: Vec<T>) -> Self {
        let selected = if items.is_empty() { None } else { Some(0) };
        Self {
            items,
            selected,
            loading: false,
            error: None,
            offset: 0,
        }
    }

    /// Set items and update selection
    pub fn set_items(&mut self, items: Vec<T>) {
        let was_empty = self.items.is_empty();
        self.items = items;
        self.loading = false;
        self.error = None;

        // Adjust selection
        if self.items.is_empty() {
            self.selected = None;
        } else if was_empty {
            self.selected = Some(0);
        } else if let Some(sel) = self.selected {
            if sel >= self.items.len() {
                self.selected = Some(self.items.len() - 1);
            }
        } else {
            self.selected = Some(0);
        }
    }

    /// Set loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
        if loading {
            self.error = None;
        }
    }

    /// Set error state
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
        self.loading = false;
    }

    /// Clear error
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Select the next item
    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = Some(match self.selected {
            Some(i) => (i + 1).min(self.items.len() - 1),
            None => 0,
        });
    }

    /// Select the previous item
    pub fn select_prev(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.selected = Some(match self.selected {
            Some(i) => i.saturating_sub(1),
            None => 0,
        });
    }

    /// Select the first item
    pub fn select_first(&mut self) {
        if !self.items.is_empty() {
            self.selected = Some(0);
        }
    }

    /// Select the last item
    pub fn select_last(&mut self) {
        if !self.items.is_empty() {
            self.selected = Some(self.items.len() - 1);
        }
    }

    /// Get the currently selected item
    pub fn selected_item(&self) -> Option<&T> {
        self.selected.and_then(|i| self.items.get(i))
    }

    /// Check if the list is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get the number of items
    pub fn len(&self) -> usize {
        self.items.len()
    }
}
