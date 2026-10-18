#![cfg(feature = "local")]
use std::sync::Arc;

use alien_bindings::providers::kv::local::LocalKv;
use alien_egress_gateway::{
    generation::GenerationFence,
    policy::{canonical, digest, Policy},
};

#[tokio::test]
async fn replicas_restarts_and_caches_require_the_exact_installed_policy() {
    let dir = tempfile::tempdir().unwrap();
    let first = GenerationFence::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    let second = GenerationFence::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    let policy = Policy::Internet;
    let hash = digest(&canonical(&policy).unwrap());
    let none_hash = digest(&canonical(&Policy::None).unwrap());
    assert!(first.resolve("p/r", "box", 0, &hash).await.is_err());
    first
        .advance("p/r", "box", 0, &policy, &hash)
        .await
        .unwrap();
    assert_eq!(
        *second.resolve("p/r", "box", 0, &hash).await.unwrap(),
        policy
    );
    first
        .advance("p/r", "box", 7, &Policy::None, &none_hash)
        .await
        .unwrap();
    assert!(second.resolve("p/r", "box", 0, &hash).await.is_err());
    assert!(second.resolve("p/r", "box", 8, &none_hash).await.is_err());
    assert!(second.resolve("p/r", "box", 7, &hash).await.is_err());
    for (generation, policy, digest) in [(3, &policy, &hash), (7, &policy, &hash)] {
        assert_eq!(
            second
                .advance("p/r", "box", generation, policy, digest)
                .await
                .unwrap_err()
                .code,
            "EGRESS_GENERATION_SUPERSEDED"
        );
    }
    second
        .advance("p/r", "box", 7, &Policy::None, &none_hash)
        .await
        .unwrap();
    assert!(second
        .resolve("p/r", "another", 7, &none_hash)
        .await
        .is_err());
    assert!(second
        .resolve("other/r", "box", 7, &none_hash)
        .await
        .is_err());
    drop(first);
    drop(second);
    let restarted = GenerationFence::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    assert!(restarted.resolve("p/r", "box", 0, &hash).await.is_err());
    assert_eq!(
        *restarted
            .resolve("p/r", "box", 7, &none_hash)
            .await
            .unwrap(),
        Policy::None
    );
}

#[tokio::test]
async fn chunks_preserve_a_policy_above_the_portable_kv_item_limit() {
    let dir = tempfile::tempdir().unwrap();
    let store = GenerationFence::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    let policy = Policy::Allow {
        allow: (0..1500)
            .map(|i| format!("{i}.{}.{}.example.com", "a".repeat(63), "b".repeat(63)))
            .collect(),
    };
    let bytes = canonical(&policy).unwrap();
    assert!(bytes.len() > 24_576);
    let hash = digest(&bytes);
    store
        .advance("p/r", "box", 0, &policy, &hash)
        .await
        .unwrap();
    assert_eq!(
        canonical(&store.resolve("p/r", "box", 0, &hash).await.unwrap()).unwrap(),
        bytes
    );
    assert!(store
        .advance("p/r", "box", 1, &Policy::None, &hash)
        .await
        .is_err());
    assert_eq!(
        canonical(&store.resolve("p/r", "box", 0, &hash).await.unwrap()).unwrap(),
        bytes
    );
    let maximum = Policy::Allow {
        allow: (0..4096).map(|i| format!("{i}.example.com")).collect(),
    };
    let max_hash = digest(&canonical(&maximum).unwrap());
    store
        .advance("p/r", "box", 1, &maximum, &max_hash)
        .await
        .unwrap();
    assert_eq!(
        canonical(&store.resolve("p/r", "box", 1, &max_hash).await.unwrap()).unwrap(),
        canonical(&maximum).unwrap()
    );
    let mut too_many = maximum;
    if let Policy::Allow { allow } = &mut too_many {
        allow.push("overflow.example.com".into());
    }
    assert_eq!(
        canonical(&too_many).unwrap_err().code,
        "SANDBOX_EGRESS_POLICY_TOO_LARGE"
    );
}
