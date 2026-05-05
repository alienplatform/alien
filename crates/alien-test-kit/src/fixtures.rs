use alien_core::{ResourceLifecycle, Stack, Storage};

/// Minimal empty stack useful for generator error tests.
pub fn empty_stack(id: impl Into<String>) -> Stack {
    Stack::new(id.into()).build()
}

/// Stack with one frozen storage resource.
pub fn single_storage_stack() -> Stack {
    let storage = Storage::new("data".to_string()).build();
    Stack::new("single-storage".to_string())
        .add(storage, ResourceLifecycle::Frozen)
        .build()
}
