//! Every check an enforcer relies on, each tried with the input a compromised
//! control plane would send.

#![allow(clippy::result_large_err)]

mod common;

use std::collections::BTreeMap;

use alien_release_trust::envelope::Envelope;
use alien_release_trust::kinds::{
    CommandBody, ComponentBody, ConfigBody, SessionBody, SessionChallenge, SessionTarget,
};
use alien_release_trust::release::RequestedSecret;
use alien_release_trust::{
    build_workload, verify_component, Floors, Kind, ReleaseBody, SignedStatement, Trust,
    WorkloadRequest, COMPONENTS_SIGNER,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use chrono::Duration;
use common::*;

fn local_inputs() -> BTreeMap<String, String> {
    BTreeMap::from([(
        "databaseUrl".to_string(),
        "postgres://db.internal/app".to_string(),
    )])
}

fn trust(keys: &Keys, chain: &[SignedStatement]) -> Trust {
    Trust::new(anchor(keys), chain).expect("the chain is valid")
}

fn code<T: std::fmt::Debug>(result: alien_release_trust::Result<T>) -> String {
    result.expect_err("must be refused").code
}

#[test]
fn a_signed_release_verifies_and_builds_exactly_what_was_signed() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);
    let verified = trust
        .verify::<ReleaseBody>(&release(&keys, 10), Kind::Release, now())
        .unwrap();
    assert_eq!(verified.sequence, 10);
    assert_eq!(verified.signers.iter().collect::<Vec<_>>(), ["ci"]);
    verified.body.validate().unwrap();

    let request = WorkloadRequest {
        env: BTreeMap::from([
            ("LOG_LEVEL".to_string(), "info".to_string()),
            ("PORT".to_string(), "8080".to_string()),
            (
                "APP_BINDING_STORE".to_string(),
                "{\"bucket\":\"b\"}".to_string(),
            ),
        ]),
        secrets: vec![RequestedSecret {
            name: "db-password".to_string(),
            env_name: Some("DB_PASSWORD".to_string()),
            file_path: None,
        }],
        cpu: Some("500m".to_string()),
        memory: Some("512Mi".to_string()),
        replicas: Some(2),
        deployer_inputs: local_inputs(),
    };
    let built = build_workload(&verified.body, "web", &BTreeMap::new(), &request).unwrap();
    assert_eq!(built.image, IMAGE);
    assert_eq!(
        built.command,
        Some(vec![
            "/app/server".to_string(),
            "--port".to_string(),
            "8080".to_string()
        ])
    );
    assert_eq!(built.env.len(), 4);
    assert_eq!(built.env["DATABASE_URL"], "postgres://db.internal/app");
    assert_eq!(built.env["APP_BINDING_STORE"], "{\"bucket\":\"b\"}");
    assert!(!built.runtime.privileged);
    assert!(built.runtime.mounts.is_empty());
    assert_eq!(built.identity.as_deref(), Some("execution"));
}

#[test]
fn every_changed_payload_byte_is_refused() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);
    let signed = release(&keys, 10);
    let bytes = signed.open().unwrap().bytes;
    for index in 0..bytes.len() {
        let mut changed = bytes.clone();
        changed[index] ^= 0x01;
        let tampered = SignedStatement {
            payload: STANDARD.encode(&changed),
            signatures: signed.signatures.clone(),
        };
        let error = trust
            .verify::<ReleaseBody>(&tampered, Kind::Release, now())
            .expect_err("a changed byte must be refused");
        assert!(
            [
                "TRUST_STATEMENT_MALFORMED",
                "TRUST_SIGNATURE_REFUSED",
                "TRUST_WRONG_KIND",
                "TRUST_WRONG_PROJECT",
                "TRUST_SIGNER_LIST_MISSING",
            ]
            .contains(&error.code.as_str()),
            "byte {index}: unexpected {}",
            error.code
        );
    }
}

#[test]
fn a_changed_signature_byte_is_refused() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);
    let signed = release(&keys, 10);
    let sig = STANDARD.decode(&signed.signatures[0].sig).unwrap();
    for index in 0..sig.len() {
        let mut changed = sig.clone();
        changed[index] ^= 0x01;
        let mut tampered = signed.clone();
        tampered.signatures[0].sig = STANDARD.encode(&changed);
        assert_eq!(
            code(trust.verify::<ReleaseBody>(&tampered, Kind::Release, now())),
            "TRUST_SIGNATURE_REFUSED",
            "signature byte {index}"
        );
    }
}

#[test]
fn statements_are_bound_to_their_kind_project_and_deployment() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);

    // A config statement presented where a release is expected.
    let config = statement(
        "config/v1",
        Some(PROJECT),
        Some(DEPLOYMENT),
        11,
        1,
        ConfigBody::default(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&config, Kind::Release, now())),
        "TRUST_WRONG_KIND"
    );
    trust
        .verify::<ConfigBody>(&config, Kind::Config, now())
        .unwrap();

    // A release body under the config type is refused by the body parser.
    let swapped = statement(
        "config/v1",
        Some(PROJECT),
        Some(DEPLOYMENT),
        12,
        1,
        release_body(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(trust.verify::<ConfigBody>(&swapped, Kind::Config, now())),
        "TRUST_BODY_REFUSED"
    );

    // The signer list itself is not a release.
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&genesis(&keys), Kind::Release, now())),
        "TRUST_WRONG_KIND"
    );

    let other_project = statement("release/v2", Some("prj_other"), None, 13, 1, release_body())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&other_project, Kind::Release, now())),
        "TRUST_WRONG_PROJECT"
    );

    let other_deployment = statement(
        "config/v1",
        Some(PROJECT),
        Some("dep_other"),
        14,
        1,
        ConfigBody::default(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(trust.verify::<ConfigBody>(&other_deployment, Kind::Config, now())),
        "TRUST_WRONG_DEPLOYMENT"
    );

    let unbound_config = statement(
        "config/v1",
        Some(PROJECT),
        None,
        15,
        1,
        ConfigBody::default(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(trust.verify::<ConfigBody>(&unbound_config, Kind::Config, now())),
        "TRUST_WRONG_DEPLOYMENT"
    );

    let release_for_one_deployment = statement(
        "release/v2",
        Some(PROJECT),
        Some(DEPLOYMENT),
        16,
        1,
        release_body(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&release_for_one_deployment, Kind::Release, now())),
        "TRUST_WRONG_DEPLOYMENT"
    );
}

/// A CI signature over bytes that need not be a valid statement.
fn signed_raw(bytes: &[u8], keys: &Keys) -> SignedStatement {
    SignedStatement {
        payload: STANDARD.encode(bytes),
        signatures: vec![alien_release_trust::Signature {
            signer: "ci".to_string(),
            sig: keys.ci.sign(bytes),
        }],
    }
}

#[test]
fn unknown_fields_are_refused_in_header_and_body() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);
    let mut header = serde_json::to_value(Envelope {
        kind: "release/v2".to_string(),
        project: Some(PROJECT.to_string()),
        deployment: None,
        sequence: 10,
        not_after: None,
        signer_list_version: 1,
        body: release_body(),
    })
    .unwrap();
    header["extra"] = serde_json::json!(true);
    let signed = signed_raw(&serde_json::to_vec(&header).unwrap(), &keys);
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&signed, Kind::Release, now())),
        "TRUST_STATEMENT_MALFORMED"
    );

    header.as_object_mut().unwrap().remove("extra");
    header["body"]["workloads"][0]["hostPid"] = serde_json::json!(true);
    let signed = SignedStatement::new(&serde_json::to_vec(&header).unwrap())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&signed, Kind::Release, now())),
        "TRUST_BODY_REFUSED"
    );

    // A duplicated header field could be read two ways; it is refused.
    let text = String::from_utf8(serde_json::to_vec(&header).unwrap()).unwrap();
    let duplicated = text.replacen("\"sequence\":10", "\"sequence\":10,\"sequence\":99", 1);
    let signed = signed_raw(duplicated.as_bytes(), &keys);
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&signed, Kind::Release, now())),
        "TRUST_STATEMENT_MALFORMED"
    );
}

#[test]
fn only_keys_on_the_list_with_the_right_role_count() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);
    let outsider = statement("release/v2", Some(PROJECT), None, 10, 1, release_body())
        .sign_with("ci", &keys.outsider)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&outsider, Kind::Release, now())),
        "TRUST_SIGNATURE_REFUSED"
    );

    let unknown_id = statement("release/v2", Some(PROJECT), None, 10, 1, release_body())
        .sign_with("mallory", &keys.outsider)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&unknown_id, Kind::Release, now())),
        "TRUST_SIGNATURE_REFUSED"
    );

    // The recovery key is an admin, not a releaser.
    let by_recovery = statement("release/v2", Some(PROJECT), None, 10, 1, release_body())
        .sign_with("recovery", &keys.recovery)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&by_recovery, Kind::Release, now())),
        "TRUST_SIGNATURE_REFUSED"
    );

    let unsigned = statement("release/v2", Some(PROJECT), None, 10, 1, release_body());
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&unsigned, Kind::Release, now())),
        "TRUST_UNSIGNED"
    );

    // A good signature beside a bad one is still refused.
    let mixed = release(&keys, 10)
        .sign_with("alice-laptop", &keys.outsider)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&mixed, Kind::Release, now())),
        "TRUST_SIGNATURE_REFUSED"
    );

    let twice = release(&keys, 10).sign_with("ci", &keys.ci).unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&twice, Kind::Release, now())),
        "TRUST_SIGNATURE_REFUSED"
    );
}

#[test]
fn the_chain_grows_only_by_admin_signed_links() {
    let keys = keys();
    let mut trust = trust(&keys, &[genesis(&keys)]);
    assert!(trust
        .extend_chain(&[genesis(&keys), version_2(&keys), version_3(&keys)])
        .unwrap());
    assert_eq!(trust.chain().current().version, 3);
    assert_eq!(trust.chain().statements().len(), 3);
    // Delivering the same chain again changes nothing.
    assert!(!trust
        .extend_chain(&[version_2(&keys), version_3(&keys)])
        .unwrap());

    // A stored chain rebuilds to the same version.
    let rebuilt = Trust::new(anchor(&keys), trust.chain().statements()).unwrap();
    assert_eq!(
        rebuilt.chain().current().digest,
        trust.chain().current().digest
    );
}

#[test]
fn chain_attacks_are_refused() {
    let keys = keys();
    let fresh = || trust(&keys, &[genesis(&keys)]);

    // Version 2 signed by a key that isn't an admin of version 1.
    let by_outsider = statement(
        "signer-list/v1",
        Some(PROJECT),
        None,
        2,
        1,
        version_2_body(&keys),
    )
    .sign_with("alice-laptop", &keys.outsider)
    .unwrap();
    assert_eq!(
        code(fresh().extend_chain(&[by_outsider])),
        "TRUST_SIGNATURE_REFUSED"
    );

    // Version 2 signed by CI, which holds no admin role.
    let by_ci = statement(
        "signer-list/v1",
        Some(PROJECT),
        None,
        2,
        1,
        version_2_body(&keys),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(fresh().extend_chain(&[by_ci])),
        "TRUST_SIGNATURE_REFUSED"
    );

    // A broken link: version 2 that doesn't name version 1's digest.
    let mut unlinked = version_2_body(&keys);
    unlinked.previous = Some(format!("sha256:{}", "0".repeat(64)));
    let unlinked = statement("signer-list/v1", Some(PROJECT), None, 2, 1, unlinked)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(fresh().extend_chain(&[unlinked])),
        "TRUST_SIGNER_LIST_REFUSED"
    );

    // A gap: version 3 without version 2.
    assert_eq!(
        code(fresh().extend_chain(&[version_3(&keys)])),
        "TRUST_SIGNER_LIST_REFUSED"
    );

    // Below threshold: version 2 needs two admins for version 3.
    let mut at_two = fresh();
    at_two.extend_chain(&[version_2(&keys)]).unwrap();
    let one_admin = statement(
        "signer-list/v1",
        Some(PROJECT),
        None,
        3,
        2,
        version_3_body(&keys),
    )
    .sign_with("alice-laptop", &keys.admin)
    .unwrap();
    assert_eq!(
        code(at_two.extend_chain(&[one_admin])),
        "TRUST_SIGNER_LIST_REFUSED"
    );

    // A fork: a different version 2 after one was trusted.
    let mut fork_body = version_2_body(&keys);
    fork_body
        .signers
        .retain(|signer| signer.id != "carol-laptop");
    fork_body.admin_threshold = 1;
    let fork = statement("signer-list/v1", Some(PROJECT), None, 2, 1, fork_body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(at_two.extend_chain(&[fork])),
        "TRUST_SIGNER_LIST_REFUSED"
    );
    assert_eq!(at_two.chain().current().version, 2);

    // An older list sent alone after a newer one is trusted changes nothing.
    assert!(!at_two.extend_chain(&[genesis(&keys)]).unwrap());
    assert_eq!(at_two.chain().current().version, 2);

    // A list for another project.
    let other = statement(
        "signer-list/v1",
        Some("prj_other"),
        None,
        2,
        1,
        version_2_body(&keys),
    )
    .sign_with("alice-laptop", &keys.admin)
    .unwrap();
    assert_eq!(code(fresh().extend_chain(&[other])), "TRUST_WRONG_PROJECT");

    // The genesis must match the pinned fingerprint.
    let mut other_genesis = genesis_body(&keys);
    other_genesis.admin_threshold = 2;
    other_genesis.signers[2]
        .roles
        .push(alien_release_trust::Role::Release);
    let other_genesis = statement("signer-list/v1", Some(PROJECT), None, 1, 1, other_genesis)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(Trust::new(anchor(&keys), &[other_genesis])),
        "TRUST_SIGNER_LIST_REFUSED"
    );
}

#[test]
fn signer_lists_with_bad_keys_or_rules_are_refused() {
    let keys = keys();
    let base = trust(&keys, &[genesis(&keys)]);
    let attempt = |body: alien_release_trust::SignerListBody| {
        let next = statement("signer-list/v1", Some(PROJECT), None, 2, 1, body)
            .sign_with("alice-laptop", &keys.admin)
            .unwrap();
        base.clone().extend_chain(&[next])
    };

    // An invalid curve point: the uncompressed point's last byte changed.
    let mut off_curve = version_2_body(&keys);
    let der = STANDARD
        .decode(
            off_curve.signers[3]
                .key
                .strip_prefix("ecdsa-p256:")
                .unwrap(),
        )
        .unwrap();
    let mut der = der;
    let last = der.len() - 1;
    der[last] ^= 0x01;
    off_curve.signers[3].key = format!("ecdsa-p256:{}", STANDARD.encode(&der));
    assert_eq!(code(attempt(off_curve)), "TRUST_SIGNER_LIST_REFUSED");

    let mut reused_key = version_2_body(&keys);
    reused_key.signers[3].key = reused_key.signers[0].key.clone();
    assert_eq!(code(attempt(reused_key)), "TRUST_SIGNER_LIST_REFUSED");

    let mut duplicate_id = version_2_body(&keys);
    duplicate_id.signers[3].id = "ci".to_string();
    assert_eq!(code(attempt(duplicate_id)), "TRUST_SIGNER_LIST_REFUSED");

    let mut threshold_too_high = version_2_body(&keys);
    threshold_too_high.admin_threshold = 4;
    assert_eq!(
        code(attempt(threshold_too_high)),
        "TRUST_SIGNER_LIST_REFUSED"
    );

    let mut no_roles = version_2_body(&keys);
    no_roles.signers[3].roles.clear();
    assert_eq!(code(attempt(no_roles)), "TRUST_SIGNER_LIST_REFUSED");
}

#[test]
fn a_removed_device_can_no_longer_sign() {
    let keys = keys();
    let mut trust = trust(&keys, &[genesis(&keys), version_2(&keys)]);
    let session = |sequence| {
        statement(
            "session/v1",
            Some(PROJECT),
            Some(DEPLOYMENT),
            sequence,
            2,
            session_body(),
        )
        .sign_with("bob-laptop", &keys.operator)
        .unwrap()
    };
    trust
        .verify::<SessionBody>(&session(1), Kind::Session, now())
        .unwrap();
    trust.extend_chain(&[version_3(&keys)]).unwrap();
    assert_eq!(
        code(trust.verify::<SessionBody>(&session(2), Kind::Session, now())),
        "TRUST_SIGNATURE_REFUSED"
    );
}

#[test]
fn a_statement_from_a_list_the_enforcer_lacks_waits_for_it() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);
    let ahead = statement("release/v2", Some(PROJECT), None, 10, 2, release_body())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&ahead, Kind::Release, now())),
        "TRUST_SIGNER_LIST_MISSING"
    );
}

#[test]
fn denied_and_expired_statements_are_refused() {
    let keys = keys();
    let bad = release(&keys, 10);
    let mut body = version_2_body(&keys);
    body.deny.push(bad.open().unwrap().digest);
    let next = statement("signer-list/v1", Some(PROJECT), None, 2, 1, body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    let trust = trust(&keys, &[genesis(&keys), next]);
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&bad, Kind::Release, now())),
        "TRUST_DENIED"
    );
    trust
        .verify::<ReleaseBody>(&release(&keys, 11), Kind::Release, now())
        .unwrap();

    let expiring = Envelope {
        kind: "release/v2".to_string(),
        project: Some(PROJECT.to_string()),
        deployment: None,
        sequence: 12,
        not_after: Some(now() - Duration::seconds(1)),
        signer_list_version: 1,
        body: release_body(),
    };
    let expired = SignedStatement::new(&expiring.to_payload().unwrap())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&expired, Kind::Release, now())),
        "TRUST_EXPIRED"
    );
    trust
        .verify::<ReleaseBody>(&expired, Kind::Release, now() - Duration::seconds(2))
        .unwrap();
}

#[test]
fn an_expired_signer_list_refuses_everything() {
    let keys = keys();
    let envelope = Envelope {
        kind: "signer-list/v1".to_string(),
        project: Some(PROJECT.to_string()),
        deployment: None,
        sequence: 2,
        not_after: Some(now() - Duration::days(1)),
        signer_list_version: 1,
        body: version_2_body(&keys),
    };
    let next = SignedStatement::new(&envelope.to_payload().unwrap())
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    let trust = trust(&keys, &[genesis(&keys), next]);
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&release(&keys, 10), Kind::Release, now())),
        "TRUST_SIGNER_LIST_REFUSED"
    );
}

#[test]
fn floors_refuse_replays_and_downgrades() {
    let mut floors = Floors::default();
    floors.check("release/v2", 10).unwrap();
    floors.raise("release/v2", 10);
    assert_eq!(
        code(floors.check("release/v2", 10)),
        "TRUST_SEQUENCE_REPLAYED"
    );
    assert_eq!(
        code(floors.check("release/v2", 9)),
        "TRUST_SEQUENCE_REPLAYED"
    );
    floors.check_current("release/v2", 10).unwrap();
    assert_eq!(
        code(floors.check_current("release/v2", 9)),
        "TRUST_SEQUENCE_REPLAYED"
    );
    floors.raise("release/v2", 5);
    assert_eq!(floors.get("release/v2"), Some(10));
    floors.check("config/v1", 1).unwrap();

    let stored = serde_json::to_string(&floors).unwrap();
    let restored: Floors = serde_json::from_str(&stored).unwrap();
    assert_eq!(restored, floors);
}

#[test]
fn control_plane_changes_to_a_workload_are_refused() {
    let release = release_body();
    release.validate().unwrap();
    let request = |env: &[(&str, &str)]| WorkloadRequest {
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        deployer_inputs: local_inputs(),
        ..WorkloadRequest::default()
    };
    let none = BTreeMap::new();

    for (env, what) in [
        (vec![("LOG_LEVEL", "debug")], "changed signed value"),
        (
            vec![("NODE_OPTIONS", "--require /tmp/x.js")],
            "loader variable",
        ),
        (vec![("LD_PRELOAD", "/tmp/x.so")], "loader variable"),
        (vec![("EXTRA", "1")], "unsigned name"),
        (
            vec![("FEATURE_FLAGS", "on")],
            "configurable name without a config statement",
        ),
    ] {
        let error = build_workload(&release, "web", &none, &request(&env)).expect_err(what);
        assert_eq!(error.code, "TRUST_WORKLOAD_REFUSED", "{what}");
    }

    // With a signed config statement the configurable value is allowed.
    let config = BTreeMap::from([("FEATURE_FLAGS".to_string(), "on".to_string())]);
    let built = build_workload(
        &release,
        "web",
        &config,
        &request(&[("FEATURE_FLAGS", "on")]),
    )
    .unwrap();
    assert_eq!(built.env["FEATURE_FLAGS"], "on");
    assert_eq!(
        code(build_workload(
            &release,
            "web",
            &config,
            &request(&[("FEATURE_FLAGS", "off")])
        )),
        "TRUST_WORKLOAD_REFUSED"
    );

    let unbound_secret = WorkloadRequest {
        secrets: vec![RequestedSecret {
            name: "db-password".to_string(),
            env_name: None,
            file_path: Some("/etc/ld.so.preload".to_string()),
        }],
        deployer_inputs: local_inputs(),
        ..WorkloadRequest::default()
    };
    assert_eq!(
        code(build_workload(&release, "web", &none, &unbound_secret)),
        "TRUST_WORKLOAD_REFUSED"
    );

    for request in [
        WorkloadRequest {
            cpu: Some("2001m".into()),
            deployer_inputs: local_inputs(),
            ..WorkloadRequest::default()
        },
        WorkloadRequest {
            memory: Some("2Gi".into()),
            deployer_inputs: local_inputs(),
            ..WorkloadRequest::default()
        },
        WorkloadRequest {
            replicas: Some(5),
            deployer_inputs: local_inputs(),
            ..WorkloadRequest::default()
        },
    ] {
        assert_eq!(
            code(build_workload(&release, "web", &none, &request)),
            "TRUST_WORKLOAD_REFUSED"
        );
    }
    assert_eq!(
        code(build_workload(
            &release,
            "other",
            &none,
            &WorkloadRequest::default()
        )),
        "TRUST_WORKLOAD_REFUSED"
    );
}

#[test]
fn deployer_inputs_come_from_the_customers_store_only() {
    let release = release_body();
    let request = |db: &str, local: Option<&str>| WorkloadRequest {
        env: BTreeMap::from([("DATABASE_URL".to_string(), db.to_string())]),
        deployer_inputs: local
            .map(|value| BTreeMap::from([("databaseUrl".to_string(), value.to_string())]))
            .unwrap_or_default(),
        ..WorkloadRequest::default()
    };
    let built = build_workload(
        &release,
        "web",
        &BTreeMap::new(),
        &request(
            "postgres://attacker.example/steal",
            Some("postgres://db.internal/app"),
        ),
    )
    .unwrap();
    assert_eq!(
        built.env["DATABASE_URL"], "postgres://db.internal/app",
        "the control plane's value is ignored"
    );
    let error = build_workload(
        &release,
        "web",
        &BTreeMap::new(),
        &request("postgres://x", None),
    )
    .expect_err("an input the customer never set is refused, not taken from the control plane");
    assert!(
        error.message.contains("not set in this deployment's store"),
        "{}",
        error.message
    );

    let mut loader = release_body();
    loader.workloads[0]
        .deployer_env
        .insert("LD_PRELOAD".into(), "preload".into());
    assert_eq!(code(loader.validate()), "TRUST_WORKLOAD_REFUSED");
}

#[test]
fn releases_that_break_the_rules_are_refused() {
    let mut unpinned = release_body();
    unpinned.workloads[0].image = "registry.example.com/app:latest".to_string();
    assert_eq!(code(unpinned.validate()), "TRUST_WORKLOAD_REFUSED");

    let mut loader_prefix = release_body();
    loader_prefix.workloads[0]
        .provided_env
        .prefixes
        .push("NODE_".to_string());
    assert_eq!(code(loader_prefix.validate()), "TRUST_WORKLOAD_REFUSED");

    let mut everything = release_body();
    everything.workloads[0]
        .provided_env
        .prefixes
        .push(String::new());
    assert_eq!(code(everything.validate()), "TRUST_WORKLOAD_REFUSED");

    let mut configurable_loader = release_body();
    configurable_loader
        .configurable
        .env
        .push("PYTHONPATH".to_string());
    assert_eq!(code(configurable_loader.validate()), "TRUST_BODY_REFUSED");

    let mut duplicate = release_body();
    duplicate.workloads.push(web_workload());
    assert_eq!(code(duplicate.validate()), "TRUST_BODY_REFUSED");

    let mut fixed_loader = release_body();
    fixed_loader.workloads[0]
        .env
        .insert("NODE_OPTIONS".into(), "--max-old-space-size=512".into());
    fixed_loader
        .validate()
        .expect("the release itself may set a loader variable");
}

#[test]
fn config_statements_set_only_configurable_names() {
    let release = release_body();
    let ok = ConfigBody {
        env: BTreeMap::from([("FEATURE_FLAGS".to_string(), "on".to_string())]),
        inputs: BTreeMap::from([("region-label".to_string(), serde_json::json!("eu"))]),
    };
    ok.check(&release).unwrap();
    for bad in [
        ConfigBody {
            env: BTreeMap::from([("NODE_OPTIONS".into(), "x".into())]),
            ..ConfigBody::default()
        },
        ConfigBody {
            env: BTreeMap::from([("LOG_LEVEL".into(), "x".into())]),
            ..ConfigBody::default()
        },
        ConfigBody {
            inputs: BTreeMap::from([("other".into(), serde_json::json!(1))]),
            ..ConfigBody::default()
        },
    ] {
        assert_eq!(code(bad.check(&release)), "TRUST_BODY_REFUSED");
    }
}

fn session_body() -> SessionBody {
    SessionBody {
        session_id: "ses_1".to_string(),
        target: SessionTarget {
            machine: Some("m-1".into()),
            workload: None,
            tool: None,
            scope: None,
        },
        argv: None,
        tty: true,
        max_duration_seconds: 1800,
        requester: "bob@example.com".to_string(),
        reason: "debugging".to_string(),
        verifier_nonce: "bm9uY2U=".to_string(),
        verifier_key: "dmVyaWZpZXI=".to_string(),
        client_key: "Y2xpZW50".to_string(),
    }
}

#[test]
fn sessions_need_the_operate_role_and_the_enforcers_own_nonce() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);
    let by_ci = statement(
        "session/v1",
        Some(PROJECT),
        Some(DEPLOYMENT),
        1,
        1,
        session_body(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(trust.verify::<SessionBody>(&by_ci, Kind::Session, now())),
        "TRUST_SIGNATURE_REFUSED"
    );

    let by_alice = statement(
        "session/v1",
        Some(PROJECT),
        Some(DEPLOYMENT),
        1,
        1,
        session_body(),
    )
    .sign_with("alice-laptop", &keys.admin)
    .unwrap();
    let verified = trust
        .verify::<SessionBody>(&by_alice, Kind::Session, now())
        .unwrap();
    let challenge = SessionChallenge {
        nonce: "bm9uY2U=".to_string(),
        verifier_key: "dmVyaWZpZXI=".to_string(),
        issued_at: now(),
        nonce_lifetime_seconds: 120,
    };
    verified
        .body
        .check(&challenge, now() + Duration::seconds(30))
        .unwrap();
    assert_eq!(
        code(
            verified
                .body
                .check(&challenge, now() + Duration::seconds(121))
        ),
        "TRUST_BODY_REFUSED"
    );
    let other_nonce = SessionChallenge {
        nonce: "b3RoZXI=".into(),
        ..challenge.clone()
    };
    assert_eq!(
        code(verified.body.check(&other_nonce, now())),
        "TRUST_BODY_REFUSED"
    );
    let other_key = SessionChallenge {
        verifier_key: "b3RoZXI=".into(),
        ..challenge.clone()
    };
    assert_eq!(
        code(verified.body.check(&other_key, now())),
        "TRUST_BODY_REFUSED"
    );
    let mut too_long = session_body();
    too_long.max_duration_seconds = 1801;
    assert_eq!(
        code(too_long.check(&challenge, now())),
        "TRUST_BODY_REFUSED"
    );
}

#[test]
fn signed_commands_bind_the_exact_parameters() {
    let release = release_body();
    let params = br#"{"key":"k1"}"#;
    let body = CommandBody {
        command: "rotate".to_string(),
        params_sha256: format!("sha256:{}", sha256_hex(params)),
        deadline: now() + Duration::minutes(5),
    };
    body.check(&release, "rotate", params, now()).unwrap();
    assert_eq!(
        code(body.check(&release, "rotate", br#"{"key":"k2"}"#, now())),
        "TRUST_BODY_REFUSED"
    );
    assert_eq!(
        code(body.check(&release, "delete", params, now())),
        "TRUST_BODY_REFUSED"
    );
    assert_eq!(
        code(body.check(&release, "rotate", params, now() + Duration::minutes(6))),
        "TRUST_BODY_REFUSED"
    );
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

#[test]
fn component_updates_verify_only_with_the_components_key() {
    let keys = keys();
    let artifact = b"agent build 42";
    let body = ComponentBody {
        name: "machine-agent".to_string(),
        version: "1.2.3".to_string(),
        target: "linux-x86_64".to_string(),
        sha256: format!("sha256:{}", sha256_hex(artifact)),
    };
    let components_key = keys.components.public_key().as_str().to_string();
    let signed = statement("component/v1", None, None, 42, 0, body.clone())
        .sign_with(COMPONENTS_SIGNER, &keys.components)
        .unwrap();
    let verified = verify_component::<ComponentBody>(&components_key, &signed, now()).unwrap();
    verified
        .body
        .check("machine-agent", "linux-x86_64", artifact)
        .unwrap();
    assert_eq!(
        code(
            verified
                .body
                .check("machine-agent", "linux-x86_64", b"other bytes")
        ),
        "TRUST_BODY_REFUSED"
    );
    assert_eq!(
        code(
            verified
                .body
                .check("machine-agent", "linux-aarch64", artifact)
        ),
        "TRUST_BODY_REFUSED"
    );

    let by_developer = statement("component/v1", None, None, 42, 0, body.clone())
        .sign_with(COMPONENTS_SIGNER, &keys.ci)
        .unwrap();
    assert_eq!(
        code(verify_component::<ComponentBody>(
            &components_key,
            &by_developer,
            now()
        )),
        "TRUST_SIGNATURE_REFUSED"
    );

    let unsigned = statement("component/v1", None, None, 42, 0, body.clone());
    assert_eq!(
        code(verify_component::<ComponentBody>(
            &components_key,
            &unsigned,
            now()
        )),
        "TRUST_UNSIGNED"
    );

    let project_bound = statement("component/v1", Some(PROJECT), None, 42, 0, body)
        .sign_with(COMPONENTS_SIGNER, &keys.components)
        .unwrap();
    assert_eq!(
        code(verify_component::<ComponentBody>(
            &components_key,
            &project_bound,
            now()
        )),
        "TRUST_BODY_REFUSED"
    );

    // A component statement is not something the signer list signs.
    let trust = trust(&keys, &[genesis(&keys)]);
    assert_eq!(
        code(trust.verify::<ComponentBody>(&signed, Kind::Component, now())),
        "TRUST_WRONG_KIND"
    );
}

#[test]
fn anchors_are_checked_when_read() {
    let keys = keys();
    let good = serde_json::to_vec(&anchor(&keys)).unwrap();
    let parsed = alien_release_trust::Anchor::parse(&good).unwrap();
    assert!(parsed.protected);

    let mut value: serde_json::Value = serde_json::from_slice(&good).unwrap();
    value["signing"] = serde_json::json!("SHA256:abc");
    assert_eq!(
        code(alien_release_trust::Anchor::parse(
            &serde_json::to_vec(&value).unwrap()
        )),
        "TRUST_ANCHOR_INVALID"
    );

    let mut value: serde_json::Value = serde_json::from_slice(&good).unwrap();
    value["components"] = serde_json::json!("ecdsa-p256:AAAA");
    assert_eq!(
        code(alien_release_trust::Anchor::parse(
            &serde_json::to_vec(&value).unwrap()
        )),
        "TRUST_ANCHOR_INVALID"
    );

    let mut value: serde_json::Value = serde_json::from_slice(&good).unwrap();
    value["protected"] = serde_json::json!("yes");
    assert_eq!(
        code(alien_release_trust::Anchor::parse(
            &serde_json::to_vec(&value).unwrap()
        )),
        "TRUST_ANCHOR_INVALID"
    );

    let mut value: serde_json::Value = serde_json::from_slice(&good).unwrap();
    value["extra"] = serde_json::json!(1);
    assert_eq!(
        code(alien_release_trust::Anchor::parse(
            &serde_json::to_vec(&value).unwrap()
        )),
        "TRUST_ANCHOR_INVALID"
    );
}
