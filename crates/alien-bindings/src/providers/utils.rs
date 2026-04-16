use object_store::path::Path;

/// Join `base` and `location` into a new `Path` without introducing extra percent-encoding.
///
/// * If `base` is empty, returns `location.clone()`.
/// * If `location` is empty, returns `base.clone()`.
/// * Otherwise, concatenates them with a single `/` separator and constructs a `Path` from the raw string.
///
/// This is preferred over `base.child(location)` when `location` may already contain
/// internal `/` segments, because `Path::child` treats the whole string as a single
/// segment and therefore encodes embedded `/` characters as `%2F`.
pub(crate) fn prefixed_path(base: &Path, location: &Path) -> Path {
    if base.as_ref().is_empty() {
        return location.clone();
    }
    if location.as_ref().is_empty() {
        return base.clone();
    }
    let joined = format!("{}/{}", base.as_ref(), location.as_ref());
    Path::from(joined)
}

/// Takes a `full_path` and attempts to make it relative to `base_dir`.
///
/// If `base_dir` is empty, `full_path` is returned as is.
/// If `base_dir` is not a prefix of `full_path` (which implies a logic error
/// if this function is used correctly), an `ObjectStoreError::Generic` is returned.
pub(crate) fn relativize_path(
    base_dir: &Path,
    full_path: Path, // Takes ownership
    store_name_for_error: &'static str,
) -> object_store::Result<Path> {
    if base_dir.as_ref().is_empty() {
        return Ok(full_path);
    }

    // Path::prefix_match consumes `full_path` if it's not found in the `match` arms,
    // so we clone it here if we need to use it in the error message later.
    // However, it's better to avoid clone if possible.
    // `prefix_match` takes `&self`, so `full_path` is not consumed by `prefix_match`.
    // It is consumed when Path::from_iter is called, or when Ok(full_path) is returned if base_dir is empty.
    match full_path.prefix_match(base_dir) {
        Some(iter) => Ok(Path::from_iter(iter)),
        None => Err(object_store::Error::Generic {
            store: store_name_for_error,
            source: format!(
                "Internal logic error: expected base_dir '{}' to be a prefix of '{}', but it was not. Cannot relativize path.",
                base_dir, full_path
            ).into(),
        }),
    }
}
