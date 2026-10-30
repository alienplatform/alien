use object_store::path::Path;

/// Join `base` and `location` into a new `Path` without introducing extra percent-encoding.
///
/// * If `base` is empty, returns `location.clone()`.
/// * If `location` is empty, returns `base.clone()`.
/// * Otherwise, joins already validated segments without encoding them again.
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
    Path::from_iter(base.parts().chain(location.parts()))
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

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;
    use crate::providers::storage::LocalStorage;
    use bytes::Bytes;
    use futures::TryStreamExt;
    use object_store::ObjectStore;

    #[tokio::test]
    async fn local_url_binding_preserves_literal_keys_through_write_list_copy_and_delete() {
        let directory = tempfile::tempdir().unwrap();
        let store =
            LocalStorage::new_from_url(&format!("file://{}/", directory.path().display())).unwrap();
        let key = "nested/quote\"é*%2F space.json";
        let path = Path::parse(key).unwrap();
        store
            .put(&path, Bytes::from_static(b"payload").into())
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(directory.path().join(key)).unwrap(),
            b"payload"
        );
        assert_eq!(
            store.get(&path).await.unwrap().bytes().await.unwrap(),
            "payload"
        );
        let listed: Vec<_> = store.list(None).try_collect().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].location.as_ref(), key);
        let target = Path::parse("copy/é%2F.json").unwrap();
        store.copy(&path, &target).await.unwrap();
        assert_eq!(
            std::fs::read(directory.path().join(target.as_ref())).unwrap(),
            b"payload"
        );
        store.delete(&path).await.unwrap();
        store.delete(&target).await.unwrap();
        assert!(store
            .list(None)
            .try_collect::<Vec<_>>()
            .await
            .unwrap()
            .is_empty());
    }
}
