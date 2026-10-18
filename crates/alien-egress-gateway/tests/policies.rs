#![cfg(feature = "local")]
use std::sync::Arc;

use alien_bindings::{
    providers::kv::local::LocalKv,
    traits::{Binding, KvEntry, PutOptions, ScanResult},
    Kv,
};
use alien_egress_gateway::{
    policy::{canonical, Policy},
    store::PolicyStore,
};
use async_trait::async_trait;
use tokio::sync::Notify;

#[tokio::test]
async fn replicas_and_restarts_observe_replacements_and_deletion_without_token_changes() {
    let dir = tempfile::tempdir().unwrap();
    let kv = Arc::new(LocalKv::new(dir.path().into()).await.unwrap());
    let first = PolicyStore::new(kv.clone());
    let second = PolicyStore::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    let closed = Policy::Allow {
        allow: vec!["blocked.example.test".into()],
    };
    assert!(first.resolve("p/r", "box").await.is_err());
    first.put("p/r", "box", 1, &Policy::Internet).await.unwrap();
    assert_eq!(
        *second.resolve("p/r", "box").await.unwrap(),
        Policy::Internet
    );
    first.put("p/r", "box", 2, &closed).await.unwrap();
    assert_eq!(*second.resolve("p/r", "box").await.unwrap(), closed);
    assert!(second.resolve("p/r", "another").await.is_err());
    assert!(second.resolve("other/r", "box").await.is_err());
    first.put("p/r", "box", 3, &Policy::Internet).await.unwrap();
    let restarted = PolicyStore::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    assert_eq!(
        *restarted.resolve("p/r", "box").await.unwrap(),
        Policy::Internet
    );
    second.delete("p/r", "box", 4).await.unwrap();
    assert!(first.resolve("p/r", "box").await.is_err());
    assert!(restarted.resolve("p/r", "box").await.is_err());
    second.delete("p/r", "box", 4).await.unwrap();
    assert_eq!(first.version("p/r", "box").await.unwrap(), 4);
}

#[tokio::test]
async fn large_policies_are_bounded_replaceable_and_corruption_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let kv = Arc::new(LocalKv::new(dir.path().into()).await.unwrap());
    let store = PolicyStore::new(kv.clone());
    let policy = Policy::Allow {
        allow: (0..1500)
            .map(|i| format!("{i}.{}.{}.example.com", "a".repeat(63), "b".repeat(63)))
            .collect(),
    };
    let bytes = canonical(&policy).unwrap();
    assert!(bytes.len() > 200_000);
    store.put("p/r", "box", 1, &policy).await.unwrap();
    assert_eq!(
        canonical(&store.resolve("p/r", "box").await.unwrap()).unwrap(),
        bytes
    );
    // Corrupt actual stored content, then use a fresh replica to force reading it.
    let entries = kv
        .scan_prefix("egress-policy:", Some(100), None)
        .await
        .unwrap();
    let chunk = entries
        .items
        .iter()
        .find(|entry| entry.key.contains(":chunk:") && entry.key.ends_with(":0"))
        .unwrap();
    kv.put(&chunk.key, b"corrupt".to_vec(), None).await.unwrap();
    let fresh = PolicyStore::new(kv.clone());
    assert!(fresh.resolve("p/r", "box").await.is_err());
    let maximum = Policy::Allow {
        allow: (0..4096).map(|i| format!("{i}.example.com")).collect(),
    };
    store.put("p/r", "box", 2, &maximum).await.unwrap();
    assert_eq!(
        canonical(&fresh.resolve("p/r", "box").await.unwrap()).unwrap(),
        canonical(&maximum).unwrap()
    );
    let Policy::Allow { mut allow } = maximum else {
        unreachable!()
    };
    allow.push("overflow.example.com".into());
    assert_eq!(
        store
            .put("p/r", "box", 3, &Policy::Allow { allow })
            .await
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_POLICY_TOO_LARGE"
    );
    store.put("p/r", "box", 3, &Policy::Internet).await.unwrap();
    let before = kv
        .scan_prefix("egress-policy:", Some(100), None)
        .await
        .unwrap()
        .items
        .len();
    store.put("p/r", "box", 3, &Policy::Internet).await.unwrap();
    assert_eq!(
        kv.scan_prefix("egress-policy:", Some(100), None)
            .await
            .unwrap()
            .items
            .len(),
        before
    );
    store.delete("p/r", "box", 4).await.unwrap();
    assert_eq!(store.version("p/r", "box").await.unwrap(), 4);
}

#[tokio::test]
async fn concurrent_replacements_publish_one_complete_policy() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(GatedKv {
        inner: LocalKv::new(dir.path().into()).await.unwrap(),
        entered: Notify::new(),
        resume: Notify::new(),
    });
    let first = PolicyStore::new(gate.clone());
    let second = PolicyStore::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    let left = Policy::Allow {
        allow: (0..4096).map(|i| format!("left-{i}.example.com")).collect(),
    };
    let right = Policy::Allow {
        allow: (0..4096)
            .map(|i| format!("right-{i}.example.com"))
            .collect(),
    };
    let write = tokio::spawn(async move { first.put("p/r", "box", 1, &left).await });
    gate.entered.notified().await;
    second.put("p/r", "box", 2, &right).await.unwrap();
    gate.resume.notify_one();
    assert_eq!(
        write.await.unwrap().unwrap_err().code,
        "EGRESS_POLICY_VERSION_CONFLICT"
    );
    let read = PolicyStore::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    let value = canonical(&read.resolve("p/r", "box").await.unwrap()).unwrap();
    assert_eq!(value, canonical(&right).unwrap());
}

#[tokio::test]
async fn stale_and_conflicting_replays_cannot_widen_or_restore_deleted_policy() {
    let dir = tempfile::tempdir().unwrap();
    let store = PolicyStore::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    assert_eq!(store.version("p/r", "box").await.unwrap(), 0);
    let narrow = Policy::Allow {
        allow: vec!["allowed.example.com".into()],
    };
    store.put("p/r", "box", 10, &narrow).await.unwrap();
    store.put("p/r", "box", 10, &narrow).await.unwrap();
    for version in [0, 9, 10] {
        assert_eq!(
            store
                .put("p/r", "box", version, &Policy::Internet)
                .await
                .unwrap_err()
                .code,
            "EGRESS_POLICY_VERSION_CONFLICT"
        );
    }
    assert_eq!(*store.resolve("p/r", "box").await.unwrap(), narrow);
    store.delete("p/r", "box", 11).await.unwrap();
    store.delete("p/r", "box", 11).await.unwrap();
    assert_eq!(store.version("p/r", "box").await.unwrap(), 11);
    for version in [10, 11] {
        assert_eq!(
            store
                .put("p/r", "box", version, &Policy::Internet)
                .await
                .unwrap_err()
                .code,
            "EGRESS_POLICY_VERSION_CONFLICT"
        );
    }
    assert!(store.resolve("p/r", "box").await.is_err());
    store
        .put("p/r", "box", 12, &Policy::Internet)
        .await
        .unwrap();
    assert_eq!(
        *store.resolve("p/r", "box").await.unwrap(),
        Policy::Internet
    );
}

/// Pause one writer immediately before publication, after another writer can replace its chunks.
#[derive(Debug)]
struct GatedKv {
    inner: LocalKv,
    entered: Notify,
    resume: Notify,
}
impl Binding for GatedKv {}
#[async_trait]
impl Kv for GatedKv {
    async fn get(&self, key: &str) -> alien_bindings::Result<Option<KvEntry>> {
        self.inner.get(key).await
    }
    async fn put(
        &self,
        key: &str,
        value: Vec<u8>,
        options: Option<PutOptions>,
    ) -> alien_bindings::Result<bool> {
        if !key.contains(":chunk:") {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        self.inner.put(key, value, options).await
    }
    async fn delete(&self, key: &str, version: Option<&str>) -> alien_bindings::Result<bool> {
        self.inner.delete(key, version).await
    }
    async fn exists(&self, key: &str) -> alien_bindings::Result<bool> {
        self.inner.exists(key).await
    }
    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> alien_bindings::Result<ScanResult> {
        self.inner.scan_prefix(prefix, limit, cursor).await
    }
}

#[tokio::test]
async fn policy_cache_keeps_hot_content_without_hiding_reference_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let kv = Arc::new(LocalKv::new(dir.path().into()).await.unwrap());
    let store = PolicyStore::new(kv.clone());
    for index in 0..128 {
        let sub = index.to_string();
        store.put("p/r", &sub, 1, &Policy::Internet).await.unwrap();
        store.resolve("p/r", &sub).await.unwrap();
    }
    store.resolve("p/r", "0").await.unwrap();
    store.put("p/r", "128", 1, &Policy::Internet).await.unwrap();
    store.resolve("p/r", "128").await.unwrap();
    // Make backing content unavailable while retaining references. A hot verified value is
    // usable; the evicted cold value must load content and fail. References are always reread.
    let mut cursor = None;
    loop {
        let page = kv.scan_prefix("egress-policy:", Some(100), cursor).await.unwrap();
        for entry in page.items {
            if entry.key.contains(":chunk:") { kv.put(&entry.key, b"corrupt".to_vec(), None).await.unwrap(); }
        }
        cursor = page.next_cursor;
        if cursor.is_none() { break; }
    }
    assert_eq!(*store.resolve("p/r", "0").await.unwrap(), Policy::Internet);
    assert!(store.resolve("p/r", "1").await.is_err());
    store.delete("p/r", "0", 2).await.unwrap();
    assert!(store.resolve("p/r", "0").await.is_err());
}
