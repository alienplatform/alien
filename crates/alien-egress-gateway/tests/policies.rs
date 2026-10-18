#![cfg(feature = "local")]
use std::sync::Arc;

use alien_bindings::{providers::kv::local::LocalKv, Kv};
use alien_egress_gateway::{
    policy::{canonical, Policy},
    store::PolicyStore,
};

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
    first.put("p/r", "box", &Policy::Internet).await.unwrap();
    assert_eq!(
        *second.resolve("p/r", "box").await.unwrap(),
        Policy::Internet
    );
    first.put("p/r", "box", &closed).await.unwrap();
    assert_eq!(*second.resolve("p/r", "box").await.unwrap(), closed);
    assert!(second.resolve("p/r", "another").await.is_err());
    assert!(second.resolve("other/r", "box").await.is_err());
    first.put("p/r", "box", &Policy::Internet).await.unwrap();
    let restarted = PolicyStore::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    assert_eq!(
        *restarted.resolve("p/r", "box").await.unwrap(),
        Policy::Internet
    );
    second.delete("p/r", "box").await.unwrap();
    assert!(first.resolve("p/r", "box").await.is_err());
    assert!(restarted.resolve("p/r", "box").await.is_err());
    second.delete("p/r", "box").await.unwrap();
    assert!(kv
        .scan_prefix("egress-policy:", None, None)
        .await
        .unwrap()
        .items
        .is_empty());
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
    store.put("p/r", "box", &policy).await.unwrap();
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
        .find(|entry| entry.key.ends_with(":chunk:0"))
        .unwrap();
    kv.put(&chunk.key, b"corrupt".to_vec(), None).await.unwrap();
    let fresh = PolicyStore::new(kv.clone());
    assert!(fresh.resolve("p/r", "box").await.is_err());
    let maximum = Policy::Allow {
        allow: (0..4096).map(|i| format!("{i}.example.com")).collect(),
    };
    store.put("p/r", "box", &maximum).await.unwrap();
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
            .put("p/r", "box", &Policy::Allow { allow })
            .await
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_POLICY_TOO_LARGE"
    );
    store.put("p/r", "box", &Policy::Internet).await.unwrap();
    assert_eq!(
        kv.scan_prefix("egress-policy:", Some(100), None)
            .await
            .unwrap()
            .items
            .len(),
        2
    );
    store.delete("p/r", "box").await.unwrap();
    assert!(kv
        .scan_prefix("egress-policy:", None, None)
        .await
        .unwrap()
        .items
        .is_empty());
}
