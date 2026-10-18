use alien_egress_gateway::{
    policy::Policy,
    record::{Claims, Record},
    store::PolicyStore,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_compact::{KeyPair, Seed};
use std::{collections::HashMap, sync::Arc};

fn record(
    version: u64,
    policy: Option<Policy>,
) -> (Record, HashMap<String, ed25519_compact::PublicKey>) {
    let pair = KeyPair::from_seed(Seed::new([7; 32]));
    let claims = Claims {
        v: 1,
        aud: "p/r".into(),
        sub: "box".into(),
        ver: (version, "0".repeat(32)),
        deleted: policy.is_none(),
        policy,
    };
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","kid":"key","typ":"alien-egress-record"}"#),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
    );
    let sig = format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(pair.sk.sign(input.as_bytes(), None).as_ref())
    );
    (
        Record {
            signed: sig,
            claims,
        },
        HashMap::from([("key".into(), pair.pk)]),
    )
}
#[tokio::test]
async fn signed_records_have_no_expiry_but_require_identity_and_signature() {
    let (record, keys) = record(1, Some(Policy::Internet));
    assert!(Record::verify(record.signed.clone(), &keys, &["p/r".into()], 1000000).is_ok());
    assert!(Record::verify(record.signed.clone(), &keys, &["other".into()], 1000000).is_err());
    let mut changed = record.signed.clone();
    changed.push('a');
    assert!(Record::verify(changed, &keys, &["p/r".into()], 1000000).is_err());
}
#[tokio::test]
async fn concurrent_signed_versions_never_regress_and_recovery_only_fills_absence() {
    let store = Arc::new(PolicyStore::new(true, 512 * 1024 * 1024));
    let (wide, _) = record(1, Some(Policy::Internet));
    let (narrow, _) = record(
        2,
        Some(Policy::Allow {
            allow: vec!["allowed.example.com".into()],
        }),
    );
    let mut tasks = tokio::task::JoinSet::new();
    for value in [narrow.clone(), wide.clone(), narrow.clone(), wide.clone()] {
        let store = store.clone();
        tasks.spawn(async move {
            store.apply(value, false).unwrap();
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
    assert_eq!(store.version("p/r", "box").await.unwrap().unwrap().0, 2);
    assert_eq!(
        *store.resolve("p/r", "box").await.unwrap(),
        narrow.claims.policy.clone().unwrap()
    );
    assert!(!store.apply(wide.clone(), false).unwrap());
    let (deleted, _) = record(3, None);
    store.apply(deleted.clone(), false).unwrap();
    assert!(store.resolve("p/r", "box").await.is_err());
    assert!(!store.apply(wide.clone(), true).unwrap());
    let restarted = PolicyStore::new(false, 512 * 1024 * 1024);
    assert!(!restarted.ready());
    for entry in store.newer_than(&[]).unwrap() {
        restarted.apply(entry, false).unwrap();
    }
    restarted.set_ready();
    assert!(restarted.ready());
    assert_eq!(restarted.version("p/r", "box").await.unwrap().unwrap().0, 3);
    assert!(restarted.resolve("p/r", "box").await.is_err());
    let empty = PolicyStore::new(true, 512 * 1024 * 1024);
    empty.apply(wide, true).unwrap();
    assert_eq!(
        *empty.resolve("p/r", "box").await.unwrap(),
        Policy::Internet
    );
    let (recreated, _) = record(4, Some(Policy::Internet));
    store.apply(recreated, false).unwrap();
    assert_eq!(
        store
            .newer_than(&restarted.summary().unwrap())
            .unwrap()
            .len(),
        1
    );
}
#[tokio::test]
async fn maximum_policy_remains_compact_and_reusable_in_memory() {
    let store = PolicyStore::new(true, 512 * 1024 * 1024);
    let policy = Policy::Allow {
        allow: (0..4096).map(|i| format!("{i}.example.com")).collect(),
    };
    let (record, keys) = record(1, Some(policy.clone()));
    Record::verify(record.signed.clone(), &keys, &["p/r".into()], 1000000).unwrap();
    store.apply(record.clone(), false).unwrap();
    store.apply(record, false).unwrap();
    assert_eq!(*store.resolve("p/r", "box").await.unwrap(), policy);
    let first = store.resolve("p/r", "box").await.unwrap();
    let second = store.resolve("p/r", "box").await.unwrap();
    assert!(Arc::ptr_eq(&first, &second));
}
