//! An extension's name, which the release sets, becomes a cache folder
//! name; a name that could leave the cache folder is refused before anything is written.

use alien_infra::extension_bundle::{testing::TestBundle, BundleCache, BundleSource};

#[tokio::test]
async fn a_name_that_could_leave_the_cache_is_refused_before_any_download() {
    let root = tempfile::tempdir().unwrap();
    let cache_dir = root.path().join("synced-extensions");
    let cache = BundleCache::new(&cache_dir);
    let bundle = TestBundle::new("acme", "1.0.0", &[]);

    assert!(cache.dir(&bundle.manifest).is_ok());

    for name in ["../escape", "a/b", "..", "/abs", "Acme", "acme\\x", ""] {
        let mut manifest = bundle.manifest.clone();
        manifest.name = name.to_string();
        assert!(cache.dir(&manifest).is_err(), "{name:?} was accepted");

        // No server listens here: a request would fail with a different error.
        let client = reqwest::Client::new();
        let error = cache
            .core(
                &manifest,
                BundleSource {
                    client: &client,
                    url: "http://127.0.0.1:9/unused",
                    bearer: None,
                },
            )
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("not lowercase letters"),
            "{name:?}: {error}"
        );
    }
    assert!(!root.path().join("escape@").exists());
    assert!(std::fs::read_dir(root.path()).unwrap().count() == 0);
}
