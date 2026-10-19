//! Independent ordering oracle for the production merge decision.
use alien_egress_gateway::record::{should_replace, Version};
use proptest::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    version: Version,
    deleted: bool,
}
fn merge(replica: &mut Option<Entry>, incoming: &Entry) {
    if should_replace(
        replica.as_ref().map(|entry| &entry.version),
        &incoming.version,
    ) {
        *replica = Some(incoming.clone());
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").map_or(32, |value| value.parse().expect("PROPTEST_CASES must be an integer")),
        failure_persistence: None,
        ..ProptestConfig::default()
    })]
    #[test]
    fn interleaved_writes_deletes_snapshots_and_catchup_converge(
        operations in prop::collection::vec((0usize..3, 0u8..5, 0u64..10000, any::<bool>()), 1..100)
    ) {
        let mut replicas: [Option<Entry>; 3] = [None, None, None];
        let mut history = Vec::new();
        for (sequence, (replica, action, millis, deleted)) in operations.into_iter().enumerate() {
            if action == 4 && replicas[replica].as_ref().is_some_and(|entry| entry.deleted) {
                // Model the documented 25-hour retention elapsing at this replica.
                // Expiry discards history; old records can then win against absence.
                // The retained-history assertions below deliberately do not claim otherwise.
                replicas[replica] = None;
            } else if action < 2 || history.is_empty() {
                // Unique writers make each generated version identify exactly one operation.
                let entry = Entry { version: (millis, format!("{sequence:032x}")), deleted };
                merge(&mut replicas[replica], &entry);
                history.push(entry);
            } else {
                // A snapshot or a catch-up page can arrive late, duplicated or out of order.
                let incoming = &history[(millis as usize) % history.len()];
                let previous = replicas[replica].clone();
                merge(&mut replicas[replica], incoming);
                if let Some(previous) = previous {
                    prop_assert!(replicas[replica].as_ref().unwrap().version >= previous.version);
                    if incoming.version <= previous.version {
                        prop_assert_eq!(&replicas[replica], &Some(previous));
                    }
                }
                let once = replicas[replica].clone();
                merge(&mut replicas[replica], incoming);
                prop_assert_eq!(&replicas[replica], &once, "equal retries must not change state");
            }
        }
        // The oracle uses Rust's pair ordering, never the production comparison.
        let expected = history.iter().max_by_key(|entry| &entry.version).unwrap();
        for replica in &mut replicas {
            for incoming in history.iter().rev() { merge(replica, incoming); }
            prop_assert_eq!(replica.as_ref(), Some(expected));
        }
        // A final deletion dominates every retained old write and snapshot.
        let deleted = Entry { version: (10001, "f".repeat(32)), deleted: true };
        for replica in &mut replicas {
            merge(replica, &deleted);
            for incoming in &history { merge(replica, incoming); }
            prop_assert_eq!(replica.as_ref(), Some(&deleted));
        }
    }
}

#[test]
fn equal_version_cannot_replace_a_tombstone_with_an_allow() {
    let version = (1, "a".repeat(32));
    let mut replica = Some(Entry {
        version: version.clone(),
        deleted: true,
    });
    merge(
        &mut replica,
        &Entry {
            version,
            deleted: false,
        },
    );
    assert!(replica.unwrap().deleted);
}
