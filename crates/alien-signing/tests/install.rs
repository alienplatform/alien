//! What one install remembers and how its enforcers find their settings,
//! each tried with what a compromised control plane, a neighbor install or a
//! fresh machine would see.

#![allow(clippy::result_large_err)]

mod common;

use std::collections::BTreeMap;

use alien_signing::{
    binding_env, build_workload, candidate_prefixes, discover, resolve_bindings, BindingContext,
    InstallId, Kind, Ledger, LocalValues, Lookup, OwnBindingFacts, RecoveryCode, Role, Signer,
    SignerKind, SigningSettings, Trust, WorkloadRequest,
};
use chrono::Duration;
use common::*;

fn code<T: std::fmt::Debug>(result: alien_signing::Result<T>) -> String {
    result.expect_err("must be refused").code
}

fn fresh_trust(keys: &Keys, chain: &[alien_signing::SignedStatement], ledger: &Ledger) -> Trust {
    Trust::new(settings(keys), install(), chain, ledger, now()).expect("the chain is valid")
}

/// Accept a release the way an enforcer does: verify, check the ledger,
/// raise it.
fn accept(trust: &Trust, ledger: &mut Ledger, sequence: u64) -> alien_signing::Result<()> {
    let keys = keys();
    let verified = trust.verify_release(&release(&keys, sequence), now())?;
    ledger.check(Kind::Release, verified.sequence, &verified.digest)?;
    ledger.raise(Kind::Release, verified.sequence, &verified.digest)?;
    trust.record(ledger)
}

#[test]
fn a_removed_signer_keeps_what_it_signed_but_signs_nothing_new() {
    let keys = keys();
    let mut ledger = Ledger::new(PREFIX);
    let mut trust = fresh_trust(&keys, &[genesis(&keys), version_2(&keys)], &ledger);
    let by_ci = release(&keys, 10);
    accept(&trust, &mut ledger, 10).unwrap();

    // Version 3 drops CI.
    let mut body = version_2_body(&keys);
    body.previous = Some(version_2(&keys).open().unwrap().digest);
    body.signers.retain(|signer| signer.id != "ci");
    let without_ci = list(PROJECT, 3, 2, body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap()
        .sign_with("carol-laptop", &keys.second_admin)
        .unwrap();
    trust.extend_chain(&[without_ci], now()).unwrap();

    let digest = by_ci.open().unwrap().digest;
    trust
        .keeps(&digest)
        .expect("what CI signed before its removal keeps running");
    assert_eq!(
        code(trust.verify_release(&release(&keys, 11), now())),
        "SIGNING_SIGNATURE_REFUSED",
        "CI can't sign a new release"
    );

    // Only the deny-list stops a running release.
    let mut body = version_2_body(&keys);
    body.previous = Some(trust.chain().current().digest.clone());
    body.signers.retain(|signer| signer.id != "ci");
    body.deny.push(digest.clone());
    let denying = list(PROJECT, 4, 3, body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap()
        .sign_with("carol-laptop", &keys.second_admin)
        .unwrap();
    trust.extend_chain(&[denying], now()).unwrap();
    assert_eq!(code(trust.keeps(&digest)), "SIGNING_DENIED");
}

#[test]
fn an_install_never_goes_back_to_an_older_release() {
    let keys = keys();
    let mut ledger = Ledger::new(PREFIX);
    let trust = fresh_trust(&keys, &[genesis(&keys)], &ledger);
    accept(&trust, &mut ledger, 10).unwrap();
    accept(&trust, &mut ledger, 12).unwrap();
    // The current release passes again; an older replica's release doesn't.
    accept(&trust, &mut ledger, 12).unwrap();
    assert_eq!(
        code(accept(&trust, &mut ledger, 11)),
        "SIGNING_SEQUENCE_REPLAYED"
    );
    assert_eq!(ledger.release.as_ref().unwrap().sequence, 12);
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
        code(Trust::new(
            settings(&keys),
            install(),
            &chain[..2],
            &seeded,
            now()
        )),
        "SIGNING_SIGNER_LIST_MISSING"
    );
    // With the full chain it gets no older release than the install had.
    let trust = fresh_trust(&keys, &chain, &seeded);
    assert_eq!(
        code(accept(&trust, &mut seeded, 11)),
        "SIGNING_SEQUENCE_REPLAYED"
    );
    accept(&trust, &mut seeded, 13).unwrap();

    // The first machine merges the newer state back: values only rise.
    first.merge(&seeded).unwrap();
    assert_eq!(first.release.as_ref().unwrap().sequence, 13);
    seeded
        .merge(&Ledger::parse(stored.as_bytes(), PREFIX).unwrap())
        .unwrap();
    assert_eq!(seeded.release.as_ref().unwrap().sequence, 13);

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
    theirs
        .raise(Kind::Release, 10, &format!("sha256:{}", "e".repeat(64)))
        .unwrap();
    assert_eq!(code(ours.merge(&theirs)), "SIGNING_LEDGER_REFUSED");
}

#[test]
fn a_withheld_config_stops_new_workloads_once_one_was_accepted() {
    let keys = keys();
    let mut ledger = Ledger::new(PREFIX);
    ledger
        .require(Kind::Config, false)
        .expect("never configured");
    let trust = fresh_trust(&keys, &[genesis(&keys)], &ledger);
    let config = statement(
        "config/v1",
        PROJECT,
        Some(INSTALL),
        3,
        1,
        alien_signing::ConfigBody::default(),
    )
    .sign_with("alice-laptop", &keys.admin)
    .unwrap();
    let verified = trust
        .verify_config(&config, &release_body(), &ledger, now())
        .unwrap();
    ledger
        .raise(Kind::Config, verified.sequence, &verified.digest)
        .unwrap();
    assert_eq!(
        code(ledger.require(Kind::Config, false)),
        "SIGNING_LEDGER_REFUSED"
    );
    ledger.require(Kind::Config, true).unwrap();
}

#[test]
fn a_config_made_from_an_older_config_is_refused() {
    let keys = keys();
    let mut ledger = Ledger::new(PREFIX);
    let trust = fresh_trust(&keys, &[genesis(&keys)], &ledger);
    let signed = |sequence: u64, base: Option<(u64, &str)>| {
        let mut body = alien_signing::ConfigBody::default();
        body.env
            .insert("LOG_LEVEL".to_string(), format!("level-{sequence}"));
        body.base = base.map(|(sequence, digest)| alien_signing::ConfigBase {
            sequence,
            digest: digest.to_string(),
        });
        statement("config/v1", PROJECT, Some(INSTALL), sequence, 1, body)
            .sign_with("alice-laptop", &keys.admin)
            .unwrap()
    };
    let mut release = release_body();
    release.configurable.env.push("LOG_LEVEL".to_string());
    let take = |statement: &alien_signing::SignedStatement, ledger: &mut Ledger| {
        let verified = trust.verify_config(statement, &release, ledger, now())?;
        ledger.raise(Kind::Config, verified.sequence, &verified.digest)?;
        Ok::<_, alien_error::AlienError<alien_signing::ErrorData>>(verified)
    };

    let first = take(&signed(1, None), &mut ledger).expect("the first config names no base");
    let first_at = (first.sequence, first.digest.as_str());
    let second = take(&signed(2, Some(first_at)), &mut ledger).expect("it builds on the first");
    let second_at = (second.sequence, second.digest.as_str());
    assert_eq!(
        code(take(&signed(3, Some(first_at)), &mut ledger)),
        "SIGNING_BODY_REFUSED",
        "a change made on the first config would undo the second"
    );
    assert_eq!(
        code(take(&signed(3, None), &mut ledger)),
        "SIGNING_BODY_REFUSED",
        "a change that names no base is refused once a config was accepted"
    );
    let other = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        code(take(
            &signed(3, Some((second.sequence, other.as_str()))),
            &mut ledger
        )),
        "SIGNING_BODY_REFUSED",
        "a base with another digest at the accepted sequence is a different config"
    );
    take(&signed(3, Some(second_at)), &mut ledger).expect("it builds on the second");
}

#[test]
fn a_config_made_on_a_config_the_install_never_accepted_is_refused() {
    let keys = keys();
    let mut ledger = Ledger::new(PREFIX);
    let trust = fresh_trust(&keys, &[genesis(&keys)], &ledger);
    let signed = |sequence: u64, base: Option<(u64, &str)>, level: &str| {
        let mut body = alien_signing::ConfigBody::default();
        body.env.insert("LOG_LEVEL".to_string(), level.to_string());
        body.base = base.map(|(sequence, digest)| alien_signing::ConfigBase {
            sequence,
            digest: digest.to_string(),
        });
        statement("config/v1", PROJECT, Some(INSTALL), sequence, 1, body)
            .sign_with("alice-laptop", &keys.admin)
            .unwrap()
    };
    let mut release = release_body();
    release.configurable.env.push("LOG_LEVEL".to_string());
    let take = |statement: &alien_signing::SignedStatement, ledger: &mut Ledger| {
        let verified = trust.verify_config(statement, &release, ledger, now())?;
        ledger.raise(Kind::Config, verified.sequence, &verified.digest)?;
        Ok::<_, alien_error::AlienError<alien_signing::ErrorData>>(verified)
    };

    // The install runs config 10. The control plane served config 5 when the
    // developer signed config 20, so 20 names base 5 and the install refuses it.
    let five = signed(5, None, "five");
    let five_at = five.open().unwrap();
    let ten = take(&signed(10, None, "ten"), &mut ledger).unwrap();
    let ten_at = (ten.sequence, ten.digest.as_str());
    let twenty = signed(20, Some((5, five_at.digest.as_str())), "twenty");
    assert_eq!(code(take(&twenty, &mut ledger)), "SIGNING_BODY_REFUSED");

    // The next edit starts from config 20, which the control plane now serves.
    // Its base is newer than what the install runs, and it copied config 20's
    // stale values, so accepting it would undo config 10.
    let twenty_at = twenty.open().unwrap();
    let twenty_one = signed(21, Some((20, twenty_at.digest.as_str())), "twenty");
    let error = take(&twenty_one, &mut ledger).unwrap_err();
    assert_eq!(code(Err::<(), _>(error.clone())), "SIGNING_BODY_REFUSED");
    assert!(
        error.message.contains("never accepted")
            && error.message.contains("runs config sequence 10")
            && error.message.contains("--from-config 10"),
        "the refusal says which config to sign from: {}",
        error.message
    );
    assert_eq!(ledger.floor(Kind::Config).unwrap().unwrap().sequence, 10);

    // Signed again from the config the install runs, it is accepted.
    take(&signed(22, Some(ten_at), "twenty-two"), &mut ledger)
        .expect("a config made from the one the install runs");
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
        code(trust.verify_config(&other, &release_body(), &Ledger::new(PREFIX), now())),
        "SIGNING_WRONG_INSTALL"
    );
}

#[test]
fn an_expired_list_blocks_new_releases_until_an_admin_refreshes_it() {
    let keys = keys();
    let later = now() + Duration::days(61);
    let trust = Trust::new(
        settings(&keys),
        install(),
        &[genesis(&keys)],
        &Ledger::new(PREFIX),
        later,
    )
    .unwrap();
    assert_eq!(
        code(trust.verify_release(&release(&keys, 10), later)),
        "SIGNING_SIGNER_LIST_REFUSED"
    );
    // What already runs is not affected.
    trust
        .keeps(&release(&keys, 9).open().unwrap().digest)
        .unwrap();

    // The admin's refresh links from the expired list.
    let mut body = genesis_body(&keys);
    body.previous = Some(genesis(&keys).open().unwrap().digest);
    let refresh = statement_until(
        "signer-list/v1",
        PROJECT,
        None,
        2,
        1,
        Some(later + Duration::days(90)),
        body.clone(),
    )
    .sign_with("alice-laptop", &keys.admin)
    .unwrap();
    let mut refreshed = trust.clone();
    refreshed.extend_chain(&[refresh], later).unwrap();
    refreshed
        .verify_release(&release(&keys, 10), later)
        .expect("releases verify again after the refresh");

    // CI can't refresh it: it holds no admin role.
    let by_ci = statement_until(
        "signer-list/v1",
        PROJECT,
        None,
        2,
        1,
        Some(later + Duration::days(90)),
        body.clone(),
    )
    .sign_with("ci", &keys.ci)
    .unwrap();
    assert_eq!(
        code(trust.clone().extend_chain(&[by_ci], later)),
        "SIGNING_SIGNATURE_REFUSED"
    );

    // A list valid for longer than the limit is refused.
    let too_long = statement_until(
        "signer-list/v1",
        PROJECT,
        None,
        2,
        1,
        Some(later + Duration::days(200)),
        body.clone(),
    )
    .sign_with("alice-laptop", &keys.admin)
    .unwrap();
    assert_eq!(
        code(trust.clone().extend_chain(&[too_long], later)),
        "SIGNING_SIGNER_LIST_REFUSED"
    );

    // And a list that never expires is refused too.
    let forever = statement("signer-list/v1", PROJECT, None, 2, 1, body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(trust.clone().extend_chain(&[forever], later)),
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
        label: None,
    });
    let unmarked = list(PROJECT, 2, 1, body.clone())
        .sign_with("recovery", &keys.recovery)
        .unwrap();
    assert_eq!(
        code(trust.clone().extend_chain(&[unmarked], now())),
        "SIGNING_SIGNER_LIST_REFUSED"
    );
    body.recovered = true;
    let marked = list(PROJECT, 2, 1, body.clone())
        .sign_with("recovery", &keys.recovery)
        .unwrap();
    let mut recovered = trust.clone();
    recovered.extend_chain(&[marked], now()).unwrap();
    assert!(recovered.chain().current().body.recovered);

    // An admin can't claim a recovery happened.
    let claimed = list(PROJECT, 2, 1, body)
        .sign_with("alice-laptop", &keys.admin)
        .unwrap();
    assert_eq!(
        code(trust.clone().extend_chain(&[claimed], now())),
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
        code(Trust::new(
            other,
            install(),
            &[genesis(&keys)],
            &ledger,
            now()
        )),
        "SIGNING_TRUST_ROOT_CHANGED"
    );
    // The install id changed, which is a new trust root too.
    let moved = InstallId::parse("aws:111122223333:eu-west-1:acme-prod").unwrap();
    assert_eq!(
        code(Trust::new(
            settings(&keys),
            moved,
            &[genesis(&keys)],
            &ledger,
            now()
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
fn the_version_a_write_was_merged_onto_is_kept_but_never_decides_a_merge() {
    let mut ours = Ledger::new(PREFIX);
    assert!(
        !ours.to_json().contains("basedOn"),
        "a store that doesn't number its writes never sees the field"
    );
    ours.raise(Kind::Release, 10, &format!("sha256:{}", "a".repeat(64)))
        .unwrap();
    ours.based_on = 7;
    let parsed = Ledger::parse(ours.to_json().as_bytes(), PREFIX).unwrap();
    assert_eq!(parsed.based_on, 7);

    let mut theirs = Ledger::new(PREFIX);
    theirs.based_on = 40;
    ours.merge(&theirs).unwrap();
    assert_eq!(ours.based_on, 7, "a merge leaves the version to the writer");
}
