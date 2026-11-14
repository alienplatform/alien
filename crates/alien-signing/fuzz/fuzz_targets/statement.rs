//! Feed arbitrary bytes to an enforcer as a statement, a signer list, signing
//! settings, a ledger and a workload request. Nothing may panic, and nothing may verify
//! unless its payload is one the fixture keys really signed: the fuzzer can
//! reorder or duplicate signatures, but it can't forge one.

#![no_main]

#[path = "../../tests/common/mod.rs"]
mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use alien_signing::session::SessionBody;
use alien_signing::{
    build_workload, ConfigBody, Kind, Ledger, LocalValues, SignedStatement, SigningSettings,
    Trust, WorkloadRequest,
};
use common::*;
use libfuzzer_sys::fuzz_target;

struct Fixture {
    trust: Trust,
    signed: BTreeSet<String>,
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let keys = keys();
        let trust = Trust::new(
            settings(&keys),
            install(),
            &[genesis(&keys), version_2(&keys)],
            &Ledger::new(PREFIX),
            now(),
        )
        .unwrap();
        // Every payload the fixture keys sign, including the corpus's.
        let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/corpus");
        let mut signed: BTreeSet<String> =
            [release(&keys, 10), release(&keys, 20), version_3(&keys)]
                .iter()
                .map(|statement| statement.open().unwrap().digest)
                .collect();
        for entry in std::fs::read_dir(corpus).unwrap() {
            let case: serde_json::Value =
                serde_json::from_slice(&std::fs::read(entry.unwrap().path()).unwrap()).unwrap();
            let statement: SignedStatement =
                serde_json::from_value(case["statement"].clone()).unwrap();
            if let Ok(opened) = statement.open() {
                signed.insert(opened.digest);
            }
        }
        Fixture { trust, signed }
    })
}

fuzz_target!(|data: &[u8]| {
    let fixture = fixture();
    let _ = SigningSettings::parse(data, PREFIX);
    if let Ok(mut ledger) = Ledger::parse(data, PREFIX) {
        let _ = ledger.merge(&Ledger::new(PREFIX));
        let _ = fixture.trust.record(&mut ledger);
    }

    // Corpus files wrap the statement in a case; accept both forms.
    let statement = serde_json::from_slice::<serde_json::Value>(data)
        .ok()
        .and_then(|value| value.get("statement").cloned().or(Some(value)))
        .and_then(|value| serde_json::from_value::<SignedStatement>(value).ok());
    let Some(statement) = statement else {
        return;
    };

    let mut trust = fixture.trust.clone();
    if trust
        .extend_chain(std::slice::from_ref(&statement), now())
        .unwrap_or(false)
    {
        let digest = statement.open().unwrap().digest;
        assert!(
            fixture.signed.contains(&digest),
            "a forged signer list was accepted"
        );
    }

    if let Ok(verified) = fixture.trust.verify_release(&statement, now()) {
        assert!(
            fixture.signed.contains(&verified.digest),
            "a forged release was accepted"
        );
        for workload in &verified.body.workloads {
            let request = WorkloadRequest {
                env: workload.env.clone(),
                ..WorkloadRequest::default()
            };
            let local = LocalValues {
                deployer_env: workload
                    .deployer_env
                    .keys()
                    .map(|name| (name.clone(), String::new()))
                    .collect(),
                bindings: workload
                    .bindings
                    .keys()
                    .map(|id| (alien_signing::binding_env_name(id), "{}".to_string()))
                    .collect(),
                own_binding: alien_signing::OwnBindingFacts::default(),
            };
            let built = build_workload(
                &verified.body,
                &workload.name,
                &BTreeMap::new(),
                &local,
                &request,
            )
            .expect("a request carrying only signed values builds");
            assert_eq!(built.image, workload.image);
        }
    }
    for kind in [Kind::Config, Kind::Session] {
        let accepted = match kind {
            Kind::Config => fixture
                .trust
                .verify::<ConfigBody>(&statement, kind, now())
                .map(|v| v.digest),
            _ => fixture
                .trust
                .verify::<SessionBody>(&statement, kind, now())
                .map(|v| v.digest),
        };
        if let Ok(digest) = accepted {
            assert!(
                fixture.signed.contains(&digest),
                "a forged {kind} was accepted"
            );
        }
    }
});
