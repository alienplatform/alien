//! Every check an enforcer relies on, each tried with the input a compromised
//! control plane would send.

#![allow(clippy::result_large_err)]

mod common;

use std::collections::BTreeMap;

use alien_signing::envelope::Envelope;
use alien_signing::release::RequestedSecret;
use alien_signing::session::{SessionBody, SessionChallenge, SessionTarget};
use alien_signing::{
    build_workload, ConfigBody, Kind, Ledger, LocalValues, ReleaseBody, SignedStatement, Trust,
    WorkloadRequest,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use chrono::Duration;
use common::*;

const UPLOADS_BINDING: &str = r#"{"bucketName":"acme-prod-uploads","service":"s3"}"#;

fn local() -> LocalValues {
    LocalValues {
        deployer_inputs: BTreeMap::from([(
            "databaseUrl".to_string(),
            "postgres://db.internal/app".to_string(),
        )]),
        bindings: BTreeMap::from([(
            "ALIEN_UPLOADS_BINDING".to_string(),
            UPLOADS_BINDING.to_string(),
        )]),
    }
}

fn trust(keys: &Keys, chain: &[SignedStatement]) -> Trust {
    Trust::new(
        settings(keys),
        install(),
        chain,
        &Ledger::new(PREFIX),
        now(),
    )
    .expect("the chain is valid")
}

fn code<T: std::fmt::Debug>(result: alien_signing::Result<T>) -> String {
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
                "ALIEN_UPLOADS_BINDING".to_string(),
                r#"{"service":"s3","bucketName":"acme-prod-uploads"}"#.to_string(),
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
    };
    let built =
        build_workload(&verified.body, "web", &BTreeMap::new(), &local(), &request).unwrap();
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
    assert_eq!(
        built.env["ALIEN_UPLOADS_BINDING"], UPLOADS_BINDING,
        "the enforcer's own value is used, whatever key order the control plane sent"
    );
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
                "SIGNING_STATEMENT_MALFORMED",
                "SIGNING_SIGNATURE_REFUSED",
                "SIGNING_WRONG_KIND",
                "SIGNING_WRONG_PROJECT",
                "SIGNING_SIGNER_LIST_MISSING",
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
            "SIGNING_SIGNATURE_REFUSED",
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
        PROJECT,
        Some(INSTALL),
        11,
        1,
        ConfigBody::default(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&config, Kind::Release, now())),
        "SIGNING_WRONG_KIND"
    );
    trust
        .verify::<ConfigBody>(&config, Kind::Config, now())
        .unwrap();

    // A release body under the config type is refused by the body parser.
    let swapped = statement("config/v1", PROJECT, Some(INSTALL), 12, 1, release_body())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ConfigBody>(&swapped, Kind::Config, now())),
        "SIGNING_BODY_REFUSED"
    );

    // The signer list itself is not a release.
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&genesis(&keys), Kind::Release, now())),
        "SIGNING_WRONG_KIND"
    );

    let other_project = statement("release/v2", "prj_other", None, 13, 1, release_body())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&other_project, Kind::Release, now())),
        "SIGNING_WRONG_PROJECT"
    );

    let other_account_install = statement(
        "config/v1",
        PROJECT,
        Some("aws:444455556666:us-east-1:acme-prod"),
        14,
        1,
        ConfigBody::default(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(trust.verify::<ConfigBody>(&other_account_install, Kind::Config, now())),
        "SIGNING_WRONG_INSTALL"
    );

    let unbound_config = statement("config/v1", PROJECT, None, 15, 1, ConfigBody::default())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ConfigBody>(&unbound_config, Kind::Config, now())),
        "SIGNING_WRONG_INSTALL"
    );

    let release_for_one_install =
        statement("release/v2", PROJECT, Some(INSTALL), 16, 1, release_body())
            .sign_with("ci", &keys.ci)
            .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&release_for_one_install, Kind::Release, now())),
        "SIGNING_WRONG_INSTALL"
    );
}

/// A CI signature over bytes that need not be a valid statement.
fn signed_raw(bytes: &[u8], keys: &Keys) -> SignedStatement {
    SignedStatement {
        payload: STANDARD.encode(bytes),
        signatures: vec![alien_signing::Signature {
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
        project: PROJECT.to_string(),
        install: None,
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
        "SIGNING_STATEMENT_MALFORMED"
    );

    header.as_object_mut().unwrap().remove("extra");
    header["body"]["workloads"][0]["hostPid"] = serde_json::json!(true);
    let signed = SignedStatement::new(&serde_json::to_vec(&header).unwrap())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&signed, Kind::Release, now())),
        "SIGNING_BODY_REFUSED"
    );

    // A duplicated header field could be read two ways; it is refused.
    let text = String::from_utf8(serde_json::to_vec(&header).unwrap()).unwrap();
    let duplicated = text.replacen("\"sequence\":10", "\"sequence\":10,\"sequence\":99", 1);
    let signed = signed_raw(duplicated.as_bytes(), &keys);
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&signed, Kind::Release, now())),
        "SIGNING_STATEMENT_MALFORMED"
    );
}

#[test]
fn only_keys_on_the_list_with_the_right_role_count() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);
    let outsider = statement("release/v2", PROJECT, None, 10, 1, release_body())
        .sign_with("ci", &keys.outsider)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&outsider, Kind::Release, now())),
        "SIGNING_SIGNATURE_REFUSED"
    );

    let unknown_id = statement("release/v2", PROJECT, None, 10, 1, release_body())
        .sign_with("mallory", &keys.outsider)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&unknown_id, Kind::Release, now())),
        "SIGNING_SIGNATURE_REFUSED"
    );

    // The recovery key is an admin, not a releaser.
    let by_recovery = statement("release/v2", PROJECT, None, 10, 1, release_body())
        .sign_with("recovery", &keys.recovery)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&by_recovery, Kind::Release, now())),
        "SIGNING_SIGNATURE_REFUSED"
    );

    let unsigned = statement("release/v2", PROJECT, None, 10, 1, release_body());
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&unsigned, Kind::Release, now())),
        "SIGNING_UNSIGNED"
    );

    // A good signature beside a bad one is still refused.
    let mixed = release(&keys, 10)
        .sign_with("alice-laptop", &keys.outsider)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&mixed, Kind::Release, now())),
        "SIGNING_SIGNATURE_REFUSED"
    );

    let twice = release(&keys, 10).sign_with("ci", &keys.ci).unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&twice, Kind::Release, now())),
        "SIGNING_SIGNATURE_REFUSED"
    );
}

#[test]
fn the_chain_grows_only_by_admin_signed_links() {
    let keys = keys();
    let mut trust = trust(&keys, &[genesis(&keys)]);
    assert!(trust
        .extend_chain(&[genesis(&keys), version_2(&keys), version_3(&keys)], now())
        .unwrap());
    assert_eq!(trust.chain().current().version, 3);
    assert_eq!(trust.chain().statements().len(), 3);
    // Delivering the same chain again changes nothing.
    assert!(!trust
        .extend_chain(&[version_2(&keys), version_3(&keys)], now())
        .unwrap());

    // A stored chain rebuilds to the same version.
    let rebuilt = Trust::new(
        settings(&keys),
        install(),
        trust.chain().statements(),
        &Ledger::new(PREFIX),
        now(),
    )
    .unwrap();
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
    let by_outsider = list(PROJECT, 2, 1, version_2_body(&keys))
        .sign_with("alice-laptop", &keys.outsider)
        .unwrap();
    assert_eq!(
        code(fresh().extend_chain(&[by_outsider], now())),
        "SIGNING_SIGNATURE_REFUSED"
    );

    // Version 2 signed by CI, which holds no admin role.
    let by_ci = list(PROJECT, 2, 1, version_2_body(&keys))
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(fresh().extend_chain(&[by_ci], now())),
        "SIGNING_SIGNATURE_REFUSED"
    );

    // A broken link: version 2 that doesn't name version 1's digest.
    let mut unlinked = version_2_body(&keys);
    unlinked.previous = Some(format!("sha256:{}", "0".repeat(64)));
    let unlinked = list(PROJECT, 2, 1, unlinked)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(fresh().extend_chain(&[unlinked], now())),
        "SIGNING_SIGNER_LIST_REFUSED"
    );

    // A gap: version 3 without version 2.
    assert_eq!(
        code(fresh().extend_chain(&[version_3(&keys)], now())),
        "SIGNING_SIGNER_LIST_REFUSED"
    );

    // Below threshold: version 2 needs two admins for version 3.
    let mut at_two = fresh();
    at_two.extend_chain(&[version_2(&keys)], now()).unwrap();
    let one_admin = list(PROJECT, 3, 2, version_3_body(&keys))
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(at_two.extend_chain(&[one_admin], now())),
        "SIGNING_SIGNER_LIST_REFUSED"
    );

    // A fork: a different version 2 after one was trusted.
    let mut fork_body = version_2_body(&keys);
    fork_body
        .signers
        .retain(|signer| signer.id != "carol-laptop");
    fork_body.admin_threshold = 1;
    let fork = list(PROJECT, 2, 1, fork_body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(at_two.extend_chain(&[fork], now())),
        "SIGNING_SIGNER_LIST_REFUSED"
    );
    assert_eq!(at_two.chain().current().version, 2);

    // An older list sent alone after a newer one is trusted changes nothing.
    assert!(!at_two.extend_chain(&[genesis(&keys)], now()).unwrap());
    assert_eq!(at_two.chain().current().version, 2);

    // A list for another project.
    let other = list("prj_other", 2, 1, version_2_body(&keys))
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(fresh().extend_chain(&[other], now())),
        "SIGNING_WRONG_PROJECT"
    );

    // The genesis must match the pinned fingerprint.
    let mut other_genesis = genesis_body(&keys);
    other_genesis.admin_threshold = 2;
    other_genesis.signers[2]
        .roles
        .push(alien_signing::Role::Release);
    let other_genesis = list(PROJECT, 1, 1, other_genesis)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(Trust::new(
            settings(&keys),
            install(),
            &[other_genesis],
            &Ledger::new(PREFIX),
            now()
        )),
        "SIGNING_SIGNER_LIST_REFUSED"
    );
}

#[test]
fn signer_lists_with_bad_keys_or_rules_are_refused() {
    let keys = keys();
    let base = trust(&keys, &[genesis(&keys)]);
    let attempt = |body: alien_signing::SignerListBody| {
        let next = list(PROJECT, 2, 1, body)
            .sign_with("alice-laptop", &keys.admin)
            .unwrap();
        base.clone().extend_chain(&[next], now())
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
    assert_eq!(code(attempt(off_curve)), "SIGNING_SIGNER_LIST_REFUSED");

    let mut reused_key = version_2_body(&keys);
    reused_key.signers[3].key = reused_key.signers[0].key.clone();
    assert_eq!(code(attempt(reused_key)), "SIGNING_SIGNER_LIST_REFUSED");

    let mut duplicate_id = version_2_body(&keys);
    duplicate_id.signers[3].id = "ci".to_string();
    assert_eq!(code(attempt(duplicate_id)), "SIGNING_SIGNER_LIST_REFUSED");

    let mut threshold_too_high = version_2_body(&keys);
    threshold_too_high.admin_threshold = 4;
    assert_eq!(
        code(attempt(threshold_too_high)),
        "SIGNING_SIGNER_LIST_REFUSED"
    );

    let mut no_roles = version_2_body(&keys);
    no_roles.signers[3].roles.clear();
    assert_eq!(code(attempt(no_roles)), "SIGNING_SIGNER_LIST_REFUSED");
}

#[test]
fn a_removed_device_can_no_longer_sign() {
    let keys = keys();
    let mut trust = trust(&keys, &[genesis(&keys), version_2(&keys)]);
    let session = |sequence| {
        statement(
            "session/v1",
            PROJECT,
            Some(INSTALL),
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
    trust.extend_chain(&[version_3(&keys)], now()).unwrap();
    assert_eq!(
        code(trust.verify::<SessionBody>(&session(2), Kind::Session, now())),
        "SIGNING_SIGNATURE_REFUSED"
    );
}

#[test]
fn a_statement_from_a_list_the_enforcer_lacks_waits_for_it() {
    let keys = keys();
    let trust = trust(&keys, &[genesis(&keys)]);
    let ahead = statement("release/v2", PROJECT, None, 10, 2, release_body())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&ahead, Kind::Release, now())),
        "SIGNING_SIGNER_LIST_MISSING"
    );
}

#[test]
fn denied_and_expired_statements_are_refused() {
    let keys = keys();
    let bad = release(&keys, 10);
    let mut body = version_2_body(&keys);
    body.deny.push(bad.open().unwrap().digest);
    let next = list(PROJECT, 2, 1, body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    let trust = trust(&keys, &[genesis(&keys), next]);
    assert_eq!(
        code(trust.verify::<ReleaseBody>(&bad, Kind::Release, now())),
        "SIGNING_DENIED"
    );
    trust
        .verify::<ReleaseBody>(&release(&keys, 11), Kind::Release, now())
        .unwrap();

    let expiring = Envelope {
        kind: "release/v2".to_string(),
        project: PROJECT.to_string(),
        install: None,
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
        "SIGNING_EXPIRED"
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
        project: PROJECT.to_string(),
        install: None,
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
        "SIGNING_SIGNER_LIST_REFUSED"
    );
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
        let error =
            build_workload(&release, "web", &none, &local(), &request(&env)).expect_err(what);
        assert_eq!(error.code, "SIGNING_WORKLOAD_REFUSED", "{what}");
    }

    // With a signed config statement the configurable value is allowed.
    let config = BTreeMap::from([("FEATURE_FLAGS".to_string(), "on".to_string())]);
    let built = build_workload(
        &release,
        "web",
        &config,
        &local(),
        &request(&[("FEATURE_FLAGS", "on")]),
    )
    .unwrap();
    assert_eq!(built.env["FEATURE_FLAGS"], "on");
    assert_eq!(
        code(build_workload(
            &release,
            "web",
            &config,
            &local(),
            &request(&[("FEATURE_FLAGS", "off")])
        )),
        "SIGNING_WORKLOAD_REFUSED"
    );

    let unbound_secret = WorkloadRequest {
        secrets: vec![RequestedSecret {
            name: "db-password".to_string(),
            env_name: None,
            file_path: Some("/etc/ld.so.preload".to_string()),
        }],
        ..WorkloadRequest::default()
    };
    assert_eq!(
        code(build_workload(
            &release,
            "web",
            &none,
            &local(),
            &unbound_secret
        )),
        "SIGNING_WORKLOAD_REFUSED"
    );

    for request in [
        WorkloadRequest {
            cpu: Some("2001m".into()),
            ..WorkloadRequest::default()
        },
        WorkloadRequest {
            memory: Some("2Gi".into()),
            ..WorkloadRequest::default()
        },
        WorkloadRequest {
            replicas: Some(5),
            ..WorkloadRequest::default()
        },
    ] {
        assert_eq!(
            code(build_workload(&release, "web", &none, &local(), &request)),
            "SIGNING_WORKLOAD_REFUSED"
        );
    }
    assert_eq!(
        code(build_workload(
            &release,
            "other",
            &none,
            &local(),
            &WorkloadRequest::default()
        )),
        "SIGNING_WORKLOAD_REFUSED"
    );
}

#[test]
fn deployer_inputs_come_from_the_customers_store_only() {
    let release = release_body();
    let request = |db: &str| WorkloadRequest {
        env: BTreeMap::from([("DATABASE_URL".to_string(), db.to_string())]),
        ..WorkloadRequest::default()
    };
    let built = build_workload(
        &release,
        "web",
        &BTreeMap::new(),
        &local(),
        &request("postgres://attacker.example/steal"),
    )
    .unwrap();
    assert_eq!(
        built.env["DATABASE_URL"], "postgres://db.internal/app",
        "the control plane's value is ignored"
    );
    let without_inputs = LocalValues {
        deployer_inputs: BTreeMap::new(),
        ..local()
    };
    let error = build_workload(
        &release,
        "web",
        &BTreeMap::new(),
        &without_inputs,
        &request("postgres://x"),
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
    assert_eq!(code(loader.validate()), "SIGNING_WORKLOAD_REFUSED");
}

#[test]
fn releases_that_break_the_rules_are_refused() {
    let mut unpinned = release_body();
    unpinned.workloads[0].image = "registry.example.com/app:latest".to_string();
    assert_eq!(code(unpinned.validate()), "SIGNING_WORKLOAD_REFUSED");

    for loader in [
        "NODE_OPTIONS",
        "LD_AUDIT_EXTRA",
        "DYLD_FRAMEWORK_PATH",
        "BASH_FUNC_ls%%",
    ] {
        let mut provided_loader = release_body();
        provided_loader.workloads[0]
            .provided_env
            .push(loader.to_string());
        assert_eq!(
            code(provided_loader.validate()),
            "SIGNING_WORKLOAD_REFUSED",
            "{loader}"
        );
    }

    let mut provided_binding = release_body();
    provided_binding.workloads[0]
        .provided_env
        .push("ALIEN_UPLOADS_BINDING".to_string());
    assert_eq!(
        code(provided_binding.validate()),
        "SIGNING_WORKLOAD_REFUSED"
    );

    let mut unknown_component = release_body();
    unknown_component
        .components
        .insert("updater".into(), format!("sha256:{}", "a".repeat(64)));
    assert_eq!(code(unknown_component.validate()), "SIGNING_BODY_REFUSED");

    let mut unknown_setup = release_body();
    unknown_setup.setup.insert(
        "pulumi".into(),
        alien_signing::SetupTemplate {
            sha256: format!("sha256:{}", "a".repeat(64)),
            allowed: Vec::new(),
        },
    );
    assert_eq!(code(unknown_setup.validate()), "SIGNING_BODY_REFUSED");

    let mut configurable_loader = release_body();
    configurable_loader
        .configurable
        .env
        .push("PYTHONPATH".to_string());
    assert_eq!(code(configurable_loader.validate()), "SIGNING_BODY_REFUSED");

    let mut duplicate = release_body();
    duplicate.workloads.push(web_workload());
    assert_eq!(code(duplicate.validate()), "SIGNING_BODY_REFUSED");

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
        telemetry_endpoint: Some("https://otel.acme.example".to_string()),
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
        ConfigBody {
            telemetry_endpoint: Some("http://collector.attacker.example".into()),
            ..ConfigBody::default()
        },
    ] {
        assert_eq!(code(bad.check(&release)), "SIGNING_BODY_REFUSED");
    }
}

fn session_body() -> SessionBody {
    SessionBody {
        session_id: "ses_1".to_string(),
        target: SessionTarget {
            machine: Some("m-1".into()),
            workload: None,
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
    let by_ci = statement("session/v1", PROJECT, Some(INSTALL), 1, 1, session_body())
        .sign_with("ci", &keys.ci)
        .unwrap();
    assert_eq!(
        code(trust.verify::<SessionBody>(&by_ci, Kind::Session, now())),
        "SIGNING_SIGNATURE_REFUSED"
    );

    let by_alice = statement("session/v1", PROJECT, Some(INSTALL), 1, 1, session_body())
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
        "SIGNING_BODY_REFUSED"
    );
    let other_nonce = SessionChallenge {
        nonce: "b3RoZXI=".into(),
        ..challenge.clone()
    };
    assert_eq!(
        code(verified.body.check(&other_nonce, now())),
        "SIGNING_BODY_REFUSED"
    );
    let other_key = SessionChallenge {
        verifier_key: "b3RoZXI=".into(),
        ..challenge.clone()
    };
    assert_eq!(
        code(verified.body.check(&other_key, now())),
        "SIGNING_BODY_REFUSED"
    );
    let mut too_long = session_body();
    too_long.max_duration_seconds = 1801;
    assert_eq!(
        code(too_long.check(&challenge, now())),
        "SIGNING_BODY_REFUSED"
    );
}
