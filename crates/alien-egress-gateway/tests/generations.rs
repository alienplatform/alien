#![cfg(feature = "local")]
use std::sync::Arc;

use alien_bindings::providers::kv::local::LocalKv;
use alien_egress_gateway::generation::GenerationFence;

#[tokio::test]
async fn replicas_and_restarts_observe_monotonic_updates() {
    let dir = tempfile::tempdir().unwrap();
    let first = GenerationFence::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    let second = GenerationFence::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    first.check("deployment/resource", "box", 0).await.unwrap();
    first
        .advance("deployment/resource", "box", 7)
        .await
        .unwrap();
    assert_eq!(
        second
            .check("deployment/resource", "box", 6)
            .await
            .unwrap_err()
            .code,
        "EGRESS_DENIED"
    );
    second
        .advance("deployment/resource", "box", 3)
        .await
        .unwrap();
    assert!(first.check("deployment/resource", "box", 6).await.is_err());
    second
        .check("deployment/resource", "another", 0)
        .await
        .unwrap();
    second.check("other/resource", "box", 0).await.unwrap();
    drop(first);
    drop(second);
    let restarted = GenerationFence::new(Arc::new(LocalKv::new(dir.path().into()).await.unwrap()));
    assert!(restarted
        .check("deployment/resource", "box", 0)
        .await
        .is_err());
    restarted
        .check("deployment/resource", "box", 7)
        .await
        .unwrap();
}
