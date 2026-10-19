#![cfg(feature = "local")]
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use alien_bindings::Bindings;
use alien_egress_gateway::{
    runtime::{self, LiveGateway},
    token,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ed25519_compact::{KeyPair, Seed};
use serde_json::json;

#[tokio::test(start_paused = true)]
async fn vault_refresh_rotates_keys_and_bounds_last_good_config() {
    let dir = tempfile::tempdir().unwrap();
    let bindings = Arc::new(Bindings::from_env_map(HashMap::from([
        ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
        ("ALIEN_SECRETS_BINDING".into(), json!({"service":"local-vault","dataDir":dir.path().join("vault"),"vaultName":"secrets"}).to_string()),
    ])).unwrap());
    let vault = bindings.vault("secrets").await.unwrap();
    let first = KeyPair::from_seed(Seed::new([1; 32]));
    let second = KeyPair::from_seed(Seed::new([2; 32]));
    let config = |pair: &KeyPair| {
        json!({"listen":"127.0.0.1:3128","admin":"127.0.0.1:3129",
        "audiences":["p/r"],"keys":[{"kid":"current","publicKey":STANDARD.encode(pair.pk.as_ref())}],
        "deniedRanges":["10.244.0.0/16"],"policyMemoryBytes":67108864})
    };
    let sign = |pair: &KeyPair| {
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"current"}"#),
            URL_SAFE_NO_PAD
                .encode(br#"{"iss":"alien-sandbox","aud":"p/r","sub":"box","iat":100,"exp":200}"#)
        );
        format!(
            "{input}.{}",
            URL_SAFE_NO_PAD.encode(pair.sk.sign(input.as_bytes(), None).as_ref())
        )
    };
    // No signing-key entry exists: the gateway must only read the gateway config entry.
    assert!(runtime::read_vault_config(&bindings).await.is_err());
    vault
        .set_secret("sandbox-egress-gateway-config", &config(&first).to_string())
        .await
        .unwrap();
    let encoded = runtime::read_vault_config(&bindings).await.unwrap();
    let accepted = Arc::new(AtomicU64::new(17));
    let live = LiveGateway::new(
        runtime::build(
            &encoded,
            bindings.clone(),
            accepted,
            Arc::new(AtomicU64::new(0)),
        )
        .await
        .unwrap(),
    );
    assert!(token::verify(&sign(&first), &live.snapshot().unwrap().keys, 100).is_ok());
    vault
        .set_secret(
            "sandbox-egress-gateway-config",
            &config(&second).to_string(),
        )
        .await
        .unwrap();
    live.refresh(&bindings).await.unwrap();
    let current = live.snapshot().unwrap();
    assert!(token::verify(&sign(&first), &current.keys, 100).is_err());
    assert!(token::verify(&sign(&second), &current.keys, 100).is_ok());
    assert_eq!(current.accepted.load(Ordering::Relaxed), 17);
    vault
        .set_secret("sandbox-egress-gateway-config", "malformed")
        .await
        .unwrap();
    assert!(live.refresh(&bindings).await.is_err());
    assert!(live.snapshot().is_some());
    vault
        .set_secret(
            "sandbox-egress-gateway-config",
            &config(&second).to_string(),
        )
        .await
        .unwrap();
    live.refresh(&bindings).await.unwrap();
    assert!(live.snapshot().is_some());
    let mut moved = config(&second);
    moved["listen"] = "127.0.0.1:3130".into();
    vault
        .set_secret("sandbox-egress-gateway-config", &moved.to_string())
        .await
        .unwrap();
    assert!(live.refresh(&bindings).await.is_err());
    assert!(live.snapshot().is_some());
    vault
        .delete_secret("sandbox-egress-gateway-config")
        .await
        .unwrap();
    assert!(live.refresh(&bindings).await.is_err());
    assert!(live.snapshot().is_some());
    tokio::time::advance(runtime::MAX_CONFIG_AGE).await;
    assert!(live.snapshot().is_none());
    for (field, value) in [
        ("policyMemoryBytes", json!(1024)),
        ("replicaSecret", json!(STANDARD.encode([7u8; 32]))),
    ] {
        let mut changed = config(&second);
        changed[field] = value;
        vault
            .set_secret("sandbox-egress-gateway-config", &changed.to_string())
            .await
            .unwrap();
        assert!(live.refresh(&bindings).await.is_err());
        assert!(live.snapshot().is_none());
    }
    vault
        .set_secret(
            "sandbox-egress-gateway-config",
            &config(&second).to_string(),
        )
        .await
        .unwrap();
    live.refresh(&bindings).await.unwrap();
    assert!(live.snapshot().is_some());
}

/// Azure links only the dedicated gateway vault; the config is read from it, and the app
/// secrets vault is not needed.
#[tokio::test]
async fn config_is_read_from_the_dedicated_gateway_vault() {
    let dir = tempfile::tempdir().unwrap();
    let bindings = Bindings::from_env_map(HashMap::from([
        ("ALIEN_DEPLOYMENT_TYPE".into(), "local".into()),
        (
            "ALIEN_SANDBOX_EGRESS_VAULT_BINDING".into(),
            json!({"service":"local-vault","dataDir":dir.path().join("vault"),"vaultName":"sandbox-egress-vault"}).to_string(),
        ),
    ]))
    .unwrap();
    bindings
        .vault("sandbox-egress-vault")
        .await
        .unwrap()
        .set_secret("sandbox-egress-gateway-config", "{\"from\":\"dedicated\"}")
        .await
        .unwrap();
    assert_eq!(
        runtime::read_vault_config(&bindings).await.unwrap(),
        "{\"from\":\"dedicated\"}"
    );
}
