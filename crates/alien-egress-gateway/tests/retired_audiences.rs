#![cfg(feature = "local")]
use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc},
};

use alien_bindings::Bindings;
use alien_egress_gateway::{
    admin,
    record::Record,
    runtime::{self, LiveGateway},
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use ed25519_compact::{KeyPair, Seed};
use serde_json::json;
use tokio::net::TcpListener;

#[tokio::test]
async fn replacement_catches_up_after_an_audience_is_retired() {
    let directory = tempfile::tempdir().unwrap();
    let bindings = Arc::new(
        Bindings::from_env_map(HashMap::from([
            ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
            (
                "ALIEN_SECRETS_BINDING".into(),
                json!({"service":"local-vault","dataDir":directory.path(),"vaultName":"secrets"})
                    .to_string(),
            ),
        ]))
        .unwrap(),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let replacement_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let replacement_address = replacement_listener.local_addr().unwrap();
    let key = KeyPair::from_seed(Seed::new([42; 32]));
    let mut config = json!({
        "listen":"127.0.0.1:3128", "admin":address, "audiences":["retired/box","active/box"],
        "keys":[{"kid":"test","publicKey":STANDARD.encode(key.pk.as_ref())}],
        "replicaSecret":STANDARD.encode([21;32]), "deniedRanges":[], "policyMemoryBytes":1048576
    });
    config["peers"] = json!({"static":[replacement_address.to_string()]});
    let source = Arc::new(LiveGateway::new(
        runtime::build(
            &config.to_string(),
            bindings.clone(),
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        )
        .await
        .unwrap(),
    ));
    for audience in ["retired/box", "active/box"] {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let payload = json!({"v":1,"aud":audience,"sub":"session","ver":[now,"00000000000000000000000000000001"],"policy":{"allow":["example.com"]}});
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"test","typ":"alien-egress-record"}"#),
            URL_SAFE_NO_PAD.encode(payload.to_string())
        );
        let signed = format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(key.sk.sign(input.as_bytes(), None).as_ref())
        );
        let gateway = source.snapshot().unwrap();
        gateway
            .policies
            .apply(
                Record::verify(signed, &gateway.keys, &gateway.config.audiences, now).unwrap(),
                false,
            )
            .unwrap();
    }
    let router = admin::router(source.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    config["audiences"] = json!(["active/box"]);
    bindings
        .vault("secrets")
        .await
        .unwrap()
        .set_secret("sandbox-egress-gateway-config", &config.to_string())
        .await
        .unwrap();
    source.refresh(&bindings).await.unwrap();
    config["peers"] = json!({"static":[address.to_string()]});
    config["admin"] = json!(replacement_address);
    let replacement = Arc::new(LiveGateway::static_config(
        runtime::build(
            &config.to_string(),
            bindings,
            Arc::new(AtomicU64::new(0)),
            Arc::new(AtomicU64::new(0)),
        )
        .await
        .unwrap(),
    ));
    let replacement_router = admin::router(replacement.clone());
    let replacement_server = tokio::spawn(async move {
        axum::serve(replacement_listener, replacement_router)
            .await
            .unwrap()
    });
    let gateway = replacement.snapshot().unwrap();
    assert!(!gateway.policies.ready());
    let result = gateway.replication.catch_up(&replacement).await;
    result.expect("retired records must not prevent a replacement becoming ready");
    assert!(gateway.policies.ready());
    assert!(gateway
        .policies
        .resolve("active/box", "session")
        .await
        .is_ok());
    assert!(gateway
        .policies
        .record("retired/box", "session")
        .unwrap()
        .is_none());
    // Catch-up in the other direction must not repeatedly push the retired record either.
    source
        .snapshot()
        .unwrap()
        .replication
        .catch_up(&source)
        .await
        .unwrap();
    assert!(source.snapshot().unwrap().policies.ready());
    assert!(gateway
        .policies
        .record("retired/box", "session")
        .unwrap()
        .is_none());
    server.abort();
    replacement_server.abort();
}
