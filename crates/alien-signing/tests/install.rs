//! What one install remembers and how its enforcers find their settings,
//! each tried with what a compromised control plane, a neighbor install or a
//! fresh machine would see.

#![allow(clippy::result_large_err)]

mod common;

use std::collections::BTreeMap;

use alien_signing::{
    binding_env, build_workload, candidate_prefixes, discover, resolve_bindings, BindingContext,
    InstallId, Ledger, LocalValues, Lookup, OwnBindingFacts, RecoveryCode, Role, Signer,
    SignerKind, SigningSettings, Trust, WorkloadRequest,
};
use chrono::Duration;
use common::*;

fn code<T: std::fmt::Debug>(result: alien_signing::Result<T>) -> String {
    result.expect_err("must be refused").code
}

fn fresh_trust(keys: &Keys, chain: &[alien_signing::SignedStatement], ledger: &Ledger) -> Trust {
    Trust::new(settings(keys), install(), chain, ledger).expect("the chain is valid")
}

/// Accept a release the way an enforcer does: verify it, then record the chain.
fn accept(trust: &Trust, ledger: &mut Ledger, sequence: u64) -> alien_signing::Result<()> {
    let keys = keys();
    let verified = trust.verify_release(&release(&keys, sequence), now())?;
    let _ = verified;
    trust.record(ledger)
}

/// A list that drops CI, signed by both admins of version 2. `keep` is what the
/// admins keep of what CI signed; `deny` what they refuse.
fn without_ci(
    keys: &Keys,
    trust: &Trust,
    deny: &[String],
    keep: &[alien_signing::Keep],
) -> alien_signing::SignedStatement {
    let current = trust.chain().current();
    let mut body = version_2_body(keys);
    body.previous = Some(current.digest.clone());
    body.signers.retain(|signer| signer.id != "ci");
    body.deny.extend(deny.iter().cloned());
    body.keep.extend(keep.iter().cloned());
    list(PROJECT, current.version + 1, current.version, body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap()
        .sign_with("carol-laptop", &keys.second_admin)
        .unwrap()
}

fn keep_of(statement: &alien_signing::SignedStatement) -> alien_signing::Keep {
    let opened = statement.open().unwrap();
    alien_signing::Keep {
        digest: opened.digest,
        kind: alien_signing::KeptKind::Release,
        release_id: Some("rel_one".to_string()),
        sequence: opened.sequence,
    }
}

#[test]
fn a_removed_signer_keeps_only_what_the_list_keeps() {
    let keys = keys();
    let ledger = Ledger::new(PREFIX);
    let trust = fresh_trust(&keys, &[genesis(&keys), version_2(&keys)], &ledger);
    let by_ci = release(&keys, 10);

    // Version 3 drops CI and keeps the release.
    let dropped = without_ci(&keys, &trust, &[], &[keep_of(&by_ci)]);
    let chain = [genesis(&keys), version_2(&keys), dropped];
    let trust = fresh_trust(&keys, &chain, &ledger);
    let base = fresh_trust(&keys, &[genesis(&keys), version_2(&keys)], &ledger);
    let verified = trust
        .verify_release(&by_ci, now())
        .expect("what the list keeps runs");
    assert!(verified.kept);
    assert_eq!(
        code(trust.verify_release(&release(&keys, 11), now())),
        "SIGNING_SIGNATURE_REFUSED",
        "CI can't sign a new release"
    );

    // Without a keep entry, everything the removed key signed is refused.
    let revoking = without_ci(&keys, &base, &[], &[]);
    let chain_no_keep = [genesis(&keys), version_2(&keys), revoking];
    let none = fresh_trust(&keys, &chain_no_keep, &ledger);
    assert_eq!(
        code(none.verify_release(&by_ci, now())),
        "SIGNING_SIGNATURE_REFUSED"
    );

    // The deny-list wins over keep.
    let digest = by_ci.open().unwrap().digest;
    let denying = without_ci(&keys, &trust, &[digest], &[keep_of(&by_ci)]);
    let chain = [genesis(&keys), version_2(&keys), chain[2].clone(), denying];
    let trust = fresh_trust(&keys, &chain, &ledger);
    assert_eq!(code(trust.verify_release(&by_ci, now())), "SIGNING_DENIED");
}

#[test]
fn a_keep_entry_binds_the_digest_kind_and_sequence() {
    let keys = keys();
    let base = fresh_trust(
        &keys,
        &[genesis(&keys), version_2(&keys)],
        &Ledger::new(PREFIX),
    );
    let by_ci = release(&keys, 10);
    let mut wrong_sequence = keep_of(&by_ci);
    wrong_sequence.sequence = 99;
    let mut wrong_kind = keep_of(&by_ci);
    wrong_kind.kind = alien_signing::KeptKind::Config;
    for entry in [wrong_sequence, wrong_kind] {
        let dropped = without_ci(&keys, &base, &[], &[entry]);
        let chain = [genesis(&keys), version_2(&keys), dropped];
        let trust = fresh_trust(&keys, &chain, &Ledger::new(PREFIX));
        assert_eq!(
            code(trust.verify_release(&by_ci, now())),
            "SIGNING_SIGNATURE_REFUSED"
        );
    }
}

#[test]
fn a_new_machine_accepts_a_kept_statement_from_the_list_alone() {
    let keys = keys();
    let base = fresh_trust(
        &keys,
        &[genesis(&keys), version_2(&keys)],
        &Ledger::new(PREFIX),
    );
    let by_ci = release(&keys, 10);
    let dropped = without_ci(&keys, &base, &[], &[keep_of(&by_ci)]);
    // A later version carries the keep along (the admin changes something else).
    let chain3 = [genesis(&keys), version_2(&keys), dropped.clone()];
    let trust3 = fresh_trust(&keys, &chain3, &Ledger::new(PREFIX));
    let mut body = trust3.chain().current().body.clone();
    body.previous = Some(trust3.chain().current().digest.clone());
    body.signers.retain(|signer| signer.id != "bob-laptop");
    body.admin_threshold = 1;
    assert_eq!(body.keep.len(), 1);
    let v4 = list(PROJECT, 4, 3, body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap()
        .sign_with("carol-laptop", &keys.second_admin)
        .unwrap();
    let chain = [genesis(&keys), version_2(&keys), dropped, v4];
    // An empty ledger, as a machine or Operator instance that never saw the release has.
    let new_machine = fresh_trust(&keys, &chain, &Ledger::new(PREFIX));
    assert!(new_machine.verify_release(&by_ci, now()).unwrap().kept);
}

#[test]
fn an_older_signed_release_may_run_again() {
    let keys = keys();
    let mut ledger = Ledger::new(PREFIX);
    let trust = fresh_trust(&keys, &[genesis(&keys)], &ledger);
    accept(&trust, &mut ledger, 12).unwrap();
    accept(&trust, &mut ledger, 11).expect("a signed rollback is allowed");
}

#[test]
fn a_new_machine_starts_from_the_install_ledger() {
    let keys = keys();
    // The first machine accepted release 12 under signer list version 3.
    let chain = [genesis(&keys), version_2(&keys), version_3(&keys)];
    let mut first = Ledger::new(PREFIX);
    let trust = fresh_trust(&keys, &chain, &first);
    accept(&trust, &mut first, 12).unwrap();
    let stored = first.to_json();

    // A machine the control plane just started reads the install's ledger.
    let mut seeded = Ledger::new(PREFIX);
    seeded
        .merge(&Ledger::parse(stored.as_bytes(), PREFIX).unwrap())
        .unwrap();
    // The control plane withholds version 3 to keep Bob, who was removed.
    assert_eq!(
        code(Trust::new(settings(&keys), install(), &chain[..2], &seeded)),
        "SIGNING_SIGNER_LIST_MISSING"
    );
    // With the full chain it accepts what the signer lists allow.
    let trust = fresh_trust(&keys, &chain, &seeded);
    accept(&trust, &mut seeded, 13).unwrap();

    // The first machine merges the newer state back: values only rise.
    first.merge(&seeded).unwrap();
    assert_eq!(first.signer_list.as_ref().unwrap().sequence, 3);

    // A ledger for a neighbor's prefix is not this install's.
    assert_eq!(
        code(Ledger::parse(stored.as_bytes(), "acme")),
        "SIGNING_LEDGER_REFUSED"
    );
}

#[test]
fn two_different_statements_with_one_sequence_are_a_fork() {
    let keys = keys();
    let mut ours = Ledger::new(PREFIX);
    let trust = fresh_trust(&keys, &[genesis(&keys)], &ours);
    accept(&trust, &mut ours, 10).unwrap();
    let mut theirs = Ledger::new(PREFIX);
    theirs.signer_list = Some(alien_signing::ledger::Entry {
        sequence: 1,
        digest: format!("sha256:{}", "e".repeat(64)),
    });
    assert_eq!(code(ours.merge(&theirs)), "SIGNING_LEDGER_REFUSED");
}

#[test]
fn a_release_for_another_channel_is_refused() {
    let keys = keys();
    let trust = fresh_trust(&keys, &[genesis(&keys)], &Ledger::new(PREFIX));
    let mut dev = release_body();
    dev.channel = "dev".to_string();
    let dev = statement("release/v2", PROJECT, None, 10, 1, dev)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(trust.verify_release(&dev, now())),
        "SIGNING_WRONG_CHANNEL"
    );
}

#[test]
fn config_for_another_customers_install_is_refused() {
    let keys = keys();
    let trust = fresh_trust(&keys, &[genesis(&keys)], &Ledger::new(PREFIX));
    // Same prefix, another account: a relabeled statement.
    let other = statement(
        "config/v1",
        PROJECT,
        Some("aws:444455556666:us-east-1:acme-prod"),
        3,
        1,
        alien_signing::ConfigBody::default(),
    )
    .sign_with("alice-laptop", &keys.admin)
    .unwrap();
    assert_eq!(
        code(trust.verify_config(&other, &release_body(), now())),
        "SIGNING_WRONG_INSTALL"
    );
}

#[test]
fn a_list_never_expires_and_one_with_an_expiry_is_refused() {
    let keys = keys();
    let trust = fresh_trust(&keys, &[genesis(&keys)], &Ledger::new(PREFIX));
    // 90 days and more later, the list still verifies new releases.
    let later = now() + Duration::days(400);
    trust
        .verify_release(&release(&keys, 10), later)
        .expect("a signer list has no lifetime");

    let mut body = genesis_body(&keys);
    body.previous = Some(genesis(&keys).open().unwrap().digest);
    let expiring = statement_until(
        "signer-list/v1",
        PROJECT,
        None,
        2,
        1,
        Some(now() + Duration::days(30)),
        body,
    )
    .sign_with("alice-laptop", &keys.admin)
    .unwrap();
    assert_eq!(
        code(trust.clone().extend_chain(&[expiring])),
        "SIGNING_SIGNER_LIST_REFUSED"
    );
}

#[test]
fn a_list_the_recovery_key_signs_says_so() {
    let keys = keys();
    let trust = fresh_trust(&keys, &[genesis(&keys)], &Ledger::new(PREFIX));
    // Alice's laptop is lost: recovery adds her new one and removes the old.
    let mut body = genesis_body(&keys);
    body.previous = Some(genesis(&keys).open().unwrap().digest);
    body.signers.retain(|signer| signer.id != "alice-laptop");
    body.signers.push(Signer {
        id: "alice-new".to_string(),
        key: keys.second_admin.public_key().as_str().to_string(),
        kind: SignerKind::Device,
        roles: vec![Role::Release, Role::Admin],
        backend: None,
        label: None,
    });
    let unmarked = list(PROJECT, 2, 1, body.clone())
        .sign_with("recovery", &keys.recovery)
        .unwrap();
    assert_eq!(
        code(trust.clone().extend_chain(&[unmarked])),
        "SIGNING_SIGNER_LIST_REFUSED"
    );
    body.recovered = true;
    let marked = list(PROJECT, 2, 1, body.clone())
        .sign_with("recovery", &keys.recovery)
        .unwrap();
    let mut recovered = trust.clone();
    recovered.extend_chain(&[marked]).unwrap();
    assert!(recovered.chain().current().body.recovered);

    // An admin can't claim a recovery happened.
    let claimed = list(PROJECT, 2, 1, body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(trust.clone().extend_chain(&[claimed])),
        "SIGNING_SIGNER_LIST_REFUSED"
    );
}

#[test]
fn the_recovery_code_derives_the_recovery_key_on_the_list() {
    let code_text = {
        let code = RecoveryCode::generate().unwrap();
        code.display()
    };
    let typed = RecoveryCode::parse(&code_text).unwrap();
    let key = typed.key(PROJECT).unwrap();
    let again = RecoveryCode::parse(&code_text.to_lowercase()).unwrap();
    assert_eq!(again.key(PROJECT).unwrap().public_key(), key.public_key());
}

#[test]
fn protection_is_sticky() {
    let keys = keys();
    let mut ledger = Ledger::new(PREFIX);
    let trust = fresh_trust(&keys, &[genesis(&keys)], &ledger);
    accept(&trust, &mut ledger, 10).unwrap();
    assert!(ledger.is_sticky());

    // The settings were deleted: nothing new runs.
    assert_eq!(
        code(ledger.check_root(None, &install())),
        "SIGNING_TRUST_ROOT_CHANGED"
    );
    // The settings now name another fingerprint.
    let mut other = settings(&keys);
    other.fingerprint = format!("SHA256:{}", "a".repeat(64));
    assert_eq!(
        code(Trust::new(other, install(), &[genesis(&keys)], &ledger)),
        "SIGNING_TRUST_ROOT_CHANGED"
    );
    // The install id changed, which is a new trust root too.
    let moved = InstallId::parse("aws:111122223333:eu-west-1:acme-prod").unwrap();
    assert_eq!(
        code(Trust::new(
            settings(&keys),
            moved,
            &[genesis(&keys)],
            &ledger
        )),
        "SIGNING_TRUST_ROOT_CHANGED"
    );
    // A ledger that never accepted anything is not sticky.
    Ledger::new(PREFIX).check_root(None, &install()).unwrap();
}

#[test]
fn finding_the_settings_never_fails_open() {
    let keys = keys();
    let ours = settings(&keys).to_json().into_bytes();
    let found = |prefix: &str, bytes: &[u8]| (prefix.to_string(), Lookup::Found(bytes.to_vec()));
    let absent = |prefix: &str| (prefix.to_string(), Lookup::Absent);
    let denied = |prefix: &str| (prefix.to_string(), Lookup::Denied);

    assert_eq!(
        discover(&[
            absent("acme-prod-verifier"),
            found(PREFIX, &ours),
            denied("acme")
        ])
        .unwrap()
        .unwrap(),
        settings(&keys)
    );
    assert!(discover(&[absent(PREFIX), denied("acme")])
        .unwrap()
        .is_none());
    // Every read refused: nothing was learned.
    assert_eq!(
        code(discover(&[denied(PREFIX), denied("acme")])),
        "SIGNING_SETTINGS_UNDECIDED"
    );
    // Two installs' settings in reach.
    let mut neighbor = settings(&keys);
    neighbor.prefix = "acme".to_string();
    assert_eq!(
        code(discover(&[
            found(PREFIX, &ours),
            found("acme", neighbor.to_json().as_bytes())
        ])),
        "SIGNING_SETTINGS_UNDECIDED"
    );
    // Settings copied from a neighbor into this install's place.
    assert_eq!(
        code(discover(&[found(PREFIX, neighbor.to_json().as_bytes())])),
        "SIGNING_SETTINGS_INVALID"
    );
    assert_eq!(
        code(discover(&[found(PREFIX, b"{\"fingerprint\":\"x\"}")])),
        "SIGNING_SETTINGS_INVALID"
    );
    assert_eq!(
        candidate_prefixes("acme-prod-verifier"),
        ["acme-prod-verifier", "acme-prod", "acme"]
    );
}

#[test]
fn settings_and_install_ids_are_checked_when_read() {
    let keys = keys();
    let good = settings(&keys);
    SigningSettings::parse(good.to_json().as_bytes(), PREFIX).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&good.to_json()).unwrap();
    value["protected"] = serde_json::json!(false);
    assert_eq!(
        code(SigningSettings::parse(value.to_string().as_bytes(), PREFIX)),
        "SIGNING_SETTINGS_INVALID"
    );
    let mut no_channel = good.clone();
    no_channel.channel = String::new();
    assert_eq!(code(no_channel.validate()), "SIGNING_SETTINGS_INVALID");

    for id in [
        "aws:111122223333:us-east-1:acme-prod",
        "gcp:acme-data-1:acme-prod",
        "azure:0b1e7d3c-5f2a-4c8e-9a11-2f3b4c5d6e7f:acme-prod-rg",
        "k8s:0b1e7d3c-5f2a-4c8e-9a11-2f3b4c5d6e7f:acme/acme-agent",
        "machine:0b1e7d3c-5f2a-4c8e-9a11-2f3b4c5d6e7f",
    ] {
        assert_eq!(InstallId::parse(id).unwrap().to_string(), id);
    }
    for id in [
        "aws:1111:us-east-1:acme",
        "aws:111122223333:us-east-1:acme:extra",
        "gcp:acme",
        "machine:not-a-uuid",
        "k8s:0b1e7d3c-5f2a-4c8e-9a11-2f3b4c5d6e7f:acme",
        "other:x",
    ] {
        assert_eq!(
            code(InstallId::parse(id)),
            "SIGNING_SETTINGS_INVALID",
            "{id}"
        );
    }
}

#[test]
fn bindings_come_from_the_install_not_the_control_plane() {
    let release = release_body();
    let context = BindingContext {
        platform: "aws".to_string(),
        prefix: PREFIX.to_string(),
        account: "111122223333".to_string(),
        region: Some("us-east-1".to_string()),
    };
    let bindings = resolve_bindings(
        binding_env(&release.workloads[0].bindings, &context).unwrap(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    )
    .unwrap();
    let local = LocalValues {
        deployer_env: BTreeMap::from([("DATABASE_URL".to_string(), "postgres://db".to_string())]),
        bindings,
        own_binding: OwnBindingFacts::default(),
    };
    let redirect = WorkloadRequest {
        env: BTreeMap::from([(
            "ALIEN_UPLOADS_BINDING".to_string(),
            r#"{"service":"s3","bucketName":"other-customer-uploads"}"#.to_string(),
        )]),
        ..WorkloadRequest::default()
    };
    assert_eq!(
        code(build_workload(
            &release,
            "web",
            &BTreeMap::new(),
            &local,
            &redirect
        )),
        "SIGNING_WORKLOAD_REFUSED"
    );
    let built = build_workload(
        &release,
        "web",
        &BTreeMap::new(),
        &local,
        &WorkloadRequest::default(),
    )
    .unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&built.env["ALIEN_UPLOADS_BINDING"]).unwrap();
    assert_eq!(
        value,
        serde_json::json!({ "service": "s3", "bucketName": "acme-prod-uploads" })
    );

    let queue = binding_env(
        &BTreeMap::from([("jobs".to_string(), "queue".to_string())]),
        &context,
    )
    .unwrap();
    assert_eq!(
        queue.values["ALIEN_JOBS_BINDING"],
        r#"{"queueUrl":"https://sqs.us-east-1.amazonaws.com/111122223333/acme-prod-jobs","service":"sqs"}"#
    );
    // A database endpoint doesn't follow from names: the enforcer reads it
    // from its own install, or a signed config carries it for an external
    // database. The control plane never does.
    let db = BTreeMap::from([("db".to_string(), "postgres".to_string())]);
    let derived = binding_env(&db, &context).unwrap();
    assert!(derived.values.is_empty());
    assert_eq!(derived.unresolved, db);
    let read = BTreeMap::from([(
        "db".to_string(),
        r#"{"host":"acme-prod-db.abc.us-east-1.rds.amazonaws.com"}"#.to_string(),
    )]);
    let signed = BTreeMap::from([(
        "db".to_string(),
        serde_json::json!({ "host": "db.vendor-hosted.example" }),
    )]);
    let from_cloud = resolve_bindings(derived.clone(), &read, &signed).unwrap();
    assert!(
        from_cloud["ALIEN_DB_BINDING"].contains("rds.amazonaws.com"),
        "what the enforcer found in its install wins over config"
    );
    let from_config = resolve_bindings(derived.clone(), &BTreeMap::new(), &signed).unwrap();
    assert!(from_config["ALIEN_DB_BINDING"].contains("vendor-hosted"));
    assert_eq!(
        code(resolve_bindings(
            derived,
            &BTreeMap::new(),
            &BTreeMap::new()
        )),
        "SIGNING_WORKLOAD_REFUSED"
    );
    // A cache endpoint follows the same rule: no names to derive it from, so a worker bound to
    // one is refused by name until the install or a signed config supplies the endpoint.
    let cache = BTreeMap::from([("cache".to_string(), "redis".to_string())]);
    let derived = binding_env(&cache, &context).unwrap();
    assert!(derived.values.is_empty());
    assert_eq!(derived.unresolved, cache);
    assert_eq!(
        code(resolve_bindings(
            derived,
            &BTreeMap::new(),
            &BTreeMap::new()
        )),
        "SIGNING_WORKLOAD_REFUSED"
    );
    // A derived value can't be overridden by config.
    let uploads = binding_env(&release.workloads[0].bindings, &context).unwrap();
    let overridden = resolve_bindings(
        uploads,
        &BTreeMap::new(),
        &BTreeMap::from([(
            "uploads".to_string(),
            serde_json::json!({"bucketName": "x"}),
        )]),
    )
    .unwrap();
    assert!(overridden["ALIEN_UPLOADS_BINDING"].contains("acme-prod-uploads"));
}

/// A GCP queue or KV a worker links to is named by setup from the install's prefix, so the
/// enforcer derives the binding. Before, both stayed unresolved and a signed worker linked to one
/// was refused on every deploy.
#[test]
fn gcp_queue_and_kv_bindings_follow_from_the_install() {
    let context = BindingContext {
        platform: "gcp".to_string(),
        prefix: PREFIX.to_string(),
        account: "acme-project".to_string(),
        region: None,
    };
    let derived = binding_env(
        &BTreeMap::from([
            ("jobs".to_string(), "queue".to_string()),
            ("metadata".to_string(), "kv".to_string()),
        ]),
        &context,
    )
    .unwrap();
    assert!(derived.unresolved.is_empty(), "{:?}", derived.unresolved);
    let value =
        |name: &str| -> serde_json::Value { serde_json::from_str(&derived.values[name]).unwrap() };
    assert_eq!(
        value("ALIEN_JOBS_BINDING"),
        serde_json::json!({
            "service": "pubsub",
            "topic": "acme-prod-jobs",
            "subscription": "acme-prod-jobs-default",
        })
    );
    assert_eq!(
        value("ALIEN_METADATA_BINDING"),
        serde_json::json!({
            "service": "firestore",
            "projectId": "acme-project",
            "databaseId": "acme-prod-metadata",
            "collectionName": "metadata",
        })
    );
}

#[test]
fn config_sets_only_bindings_the_release_has() {
    let release = release_body();
    let ok = alien_signing::ConfigBody {
        bindings: BTreeMap::from([("uploads".to_string(), serde_json::json!({}))]),
        ..alien_signing::ConfigBody::default()
    };
    ok.check(&release).unwrap();
    let other = alien_signing::ConfigBody {
        bindings: BTreeMap::from([("payments".to_string(), serde_json::json!({}))]),
        ..alien_signing::ConfigBody::default()
    };
    assert_eq!(code(other.check(&release)), "SIGNING_BODY_REFUSED");
}

#[test]
fn a_live_gcp_queue_and_kv_are_named_the_way_their_controllers_name_them() {
    let context = BindingContext {
        platform: "gcp".to_string(),
        prefix: PREFIX.to_string(),
        account: "acme-project".to_string(),
        region: None,
    };
    let live = std::collections::BTreeSet::from(["jobs".to_string(), "Meta_data".to_string()]);
    let derived = alien_signing::binding_env_with(
        &BTreeMap::from([
            ("jobs".to_string(), "queue".to_string()),
            ("Meta_data".to_string(), "kv".to_string()),
        ]),
        &live,
        &context,
    )
    .unwrap();
    let value =
        |name: &str| -> serde_json::Value { serde_json::from_str(&derived.values[name]).unwrap() };
    assert_eq!(
        value("ALIEN_JOBS_BINDING"),
        serde_json::json!({
            "service": "pubsub",
            "topic": "acme-prod-jobs",
            "subscription": "acme-prod-jobs-sub",
        })
    );
    assert_eq!(
        value("ALIEN_META_DATA_BINDING"),
        serde_json::json!({
            "service": "firestore",
            "projectId": "acme-project",
            "databaseId": "(default)",
            "collectionName": "kv-acme-prod-meta-data",
        })
    );
}

#[test]
fn an_older_list_with_the_removed_key_is_refused_by_the_version_floor() {
    let keys = keys();
    let mut ledger = Ledger::new(PREFIX);
    let base = [genesis(&keys), version_2(&keys)];
    let trust = fresh_trust(&keys, &base, &ledger);
    let by_ci = release(&keys, 10);
    let dropped = without_ci(&keys, &trust, &[], &[keep_of(&by_ci)]);
    let chain = [base[0].clone(), base[1].clone(), dropped];
    let current = fresh_trust(&keys, &chain, &ledger);
    current.record(&mut ledger).unwrap();

    // Replaying the older list, which still has CI, is refused: the install
    // accepted version 3.
    let error = Trust::new(settings(&keys), install(), &base, &ledger)
        .expect_err("the older list must be refused");
    assert_eq!(error.code, "SIGNING_SIGNER_LIST_MISSING", "{error:?}");

    // CI's new signatures are refused on the list that dropped it.
    assert_eq!(
        code(current.verify_release(&release(&keys, 11), now())),
        "SIGNING_SIGNATURE_REFUSED"
    );
    // And what the list keeps still runs.
    current.verify_release(&by_ci, now()).unwrap();
}

#[test]
fn a_keep_entry_for_another_install_or_project_lets_nothing_through() {
    let keys = keys();
    let ledger = Ledger::new(PREFIX);
    let base = fresh_trust(&keys, &[genesis(&keys), version_2(&keys)], &ledger);
    let other_install = statement(
        "config/v1",
        PROJECT,
        Some("aws:444455556666:us-east-1:acme-prod"),
        3,
        1,
        alien_signing::ConfigBody::default(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    let other_project = statement("release/v2", "prj_other", None, 12, 1, release_body())
        .sign_with("ci", &keys.ci)
        .unwrap();
    let mut kept_config = keep_of(&other_install);
    kept_config.kind = alien_signing::KeptKind::Config;
    let dropped = without_ci(&keys, &base, &[], &[kept_config, keep_of(&other_project)]);
    let chain = [genesis(&keys), version_2(&keys), dropped];
    let trust = fresh_trust(&keys, &chain, &ledger);
    assert_eq!(
        code(trust.verify_config(&other_install, &release_body(), now())),
        "SIGNING_WRONG_INSTALL"
    );
    assert_eq!(
        code(trust.verify_release(&other_project, now())),
        "SIGNING_WRONG_PROJECT"
    );
}
