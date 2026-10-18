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
    record_for(version, policy, "box")
}
fn record_for(
    version: u64,
    policy: Option<Policy>,
    sub: &str,
) -> (Record, HashMap<String, ed25519_compact::PublicKey>) {
    let pair = KeyPair::from_seed(Seed::new([7; 32]));
    let claims = Claims {
        v: 1,
        aud: "p/r".into(),
        sub: sub.into(),
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
    let now = chrono::Utc::now().timestamp_millis() as u64;
    let store = Arc::new(PolicyStore::new(true, 512 * 1024 * 1024));
    let (wide, _) = record(now, Some(Policy::Internet));
    let (narrow, _) = record(
        now + 1,
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
    assert_eq!(
        store.version("p/r", "box").await.unwrap().unwrap().0,
        now + 1
    );
    assert_eq!(
        *store.resolve("p/r", "box").await.unwrap(),
        narrow.claims.policy.clone().unwrap()
    );
    assert!(!store.apply(wide.clone(), false).unwrap());
    let (deleted, _) = record(now + 2, None);
    store.apply(deleted.clone(), false).unwrap();
    assert!(store.resolve("p/r", "box").await.is_err());
    assert!(!store.apply(wide.clone(), true).unwrap());
    let restarted = PolicyStore::new(false, 512 * 1024 * 1024);
    assert!(!restarted.ready());
    for entry in store.newer_than(&[], &["p/r".into()]).unwrap() {
        restarted.apply(entry.as_ref().clone(), false).unwrap();
    }
    restarted.set_ready();
    assert!(restarted.ready());
    assert_eq!(
        restarted.version("p/r", "box").await.unwrap().unwrap().0,
        now + 2
    );
    assert!(restarted.resolve("p/r", "box").await.is_err());
    let empty = PolicyStore::new(true, 512 * 1024 * 1024);
    empty.apply(wide, true).unwrap();
    assert_eq!(
        *empty.resolve("p/r", "box").await.unwrap(),
        Policy::Internet
    );
    let (recreated, _) = record(now + 3, Some(Policy::Internet));
    store.apply(recreated, false).unwrap();
    assert_eq!(
        store
            .newer_than(&restarted.summary().unwrap(), &["p/r".into()])
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

#[tokio::test]
async fn memory_overflow_preserves_the_previous_enforced_policy() {
    let store = PolicyStore::new(true, 2048);
    let (live, _) = record(1, Some(Policy::Allow { allow: vec![] }));
    store.apply(live, false).unwrap();
    let (larger, _) = record(
        2,
        Some(Policy::Allow {
            allow: (0..256).map(|i| format!("{i}.example.com")).collect(),
        }),
    );
    assert_eq!(
        store.apply(larger, false).unwrap_err().code,
        "SANDBOX_EGRESS_POLICY_STORE_FULL"
    );
    assert_eq!(
        *store.resolve("p/r", "box").await.unwrap(),
        Policy::Allow { allow: vec![] }
    );
}

#[test]
fn future_clock_guard_and_writer_ties_use_the_signed_version() {
    let (future, keys) = record(61_001, Some(Policy::Internet));
    assert_eq!(
        Record::verify(future.signed.clone(), &keys, &["p/r".into()], 1_000)
            .err()
            .unwrap()
            .code,
        "SANDBOX_EGRESS_VERSION_IN_FUTURE"
    );
    assert!(Record::verify(future.signed, &keys, &["p/r".into()], 1_001).is_ok());
    use alien_egress_gateway::record::should_replace;
    let lower = (100, "0".repeat(32));
    let higher = (100, "1".repeat(32));
    assert!(should_replace(Some(&lower), &higher));
    assert!(!should_replace(Some(&higher), &lower));
    assert!(!should_replace(Some(&higher), &higher));
}

#[tokio::test]
async fn bounded_snapshot_pages_restore_every_large_signed_policy() {
    let source = PolicyStore::new(true, 64 * 1024 * 1024);
    let restored = PolicyStore::new(false, 64 * 1024 * 1024);
    let policy = Policy::Allow {
        allow: (0..4000)
            .map(|i| format!("{i}.{}.example.com", "a".repeat(42)))
            .collect(),
    };
    let mut accepted = 0;
    loop {
        let (entry, keys) = record_for(1, Some(policy.clone()), &format!("box-{accepted}"));
        let mut verified = Record::verify(entry.signed, &keys, &["p/r".into()], 1000).unwrap();
        // Transport buffers may leave spare capacity without changing signed bytes.
        verified.signed.reserve(12345);
        match source.apply(verified, false) {
            Ok(true) => accepted += 1,
            Err(error) => {
                assert_eq!(error.code, "SANDBOX_EGRESS_POLICY_STORE_FULL");
                break;
            }
            Ok(false) => panic!("new identity must be accepted or rejected for capacity"),
        }
        assert!(accepted < 100, "fixture must fill the bounded store");
    }
    assert!(accepted >= 30, "fixture must span multiple snapshot pages");
    let mut pages = 0;
    loop {
        let page = source
            .newer_than(&restored.summary().unwrap(), &["p/r".into()])
            .unwrap();
        if page.is_empty() {
            break;
        }
        assert!(
            page.iter()
                .map(|entry| entry.signed.len() + 4)
                .sum::<usize>()
                <= 8 * 1024 * 1024
        );
        pages += 1;
        for entry in page {
            let keys = record(1, Some(Policy::Internet)).1;
            let verified =
                Record::verify(entry.signed.clone(), &keys, &["p/r".into()], 1000).unwrap();
            restored.apply(verified, false).unwrap();
        }
    }
    assert!(pages > 1);
    assert_eq!(restored.summary().unwrap().len(), accepted);
    let extra = record_for(1, Some(policy.clone()), "one-too-many").0;
    assert_eq!(
        restored.apply(extra, false).unwrap_err().code,
        "SANDBOX_EGRESS_POLICY_STORE_FULL"
    );
    for index in 0..accepted {
        assert_eq!(
            *restored
                .resolve("p/r", &format!("box-{index}"))
                .await
                .unwrap(),
            policy
        );
    }
}

#[test]
fn cgroup_budget_requires_a_finite_limit_and_rejects_records_past_half_of_it() {
    use alien_egress_gateway::memory::policy_budget;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.max");
    assert!(policy_budget(None, &path).is_err());
    std::fs::write(&path, "max\n").unwrap();
    assert!(policy_budget(None, &path).is_err());
    assert_eq!(policy_budget(Some(1024), &path).unwrap(), 1024);
    assert!(policy_budget(Some(0), &path).is_err());
    std::fs::write(&path, "1024\n").unwrap();
    let budget = policy_budget(None, &path).unwrap();
    assert_eq!(budget, 512);
    let store = PolicyStore::new(true, budget);
    assert_eq!(
        store
            .apply(record(1, Some(Policy::Internet)).0, false)
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_POLICY_STORE_FULL"
    );
    assert!(store.record("p/r", "box").unwrap().is_none());
}
