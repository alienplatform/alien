//! `verify` against what a protected stack and its control plane return,
//! and against what a tampered stack or a lying control plane return.

use super::*;
use alien_cli_common::setup_templates::{
    render_setup_template, RenderedSetupTemplate, SetupHostingOptions, SetupRenderOptions,
    RENDERER_VERSION,
};
use alien_signing::{
    Envelope, Role, SecretKey, SetupRenderer, SetupTemplate, Signer, SignerListBody,
};
use chrono::Duration;

const PREFIX: &str = "acme-prod";
const STACK_ID: &str =
    "arn:aws:cloudformation:us-east-1:111122223333:stack/acme-prod/0f3a5c00-9b1e-11ef-8a4b-0e5d2f6a1b27";
pub(crate) const PROJECT: &str = "prj_acme";

pub(crate) fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-05T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

pub(crate) fn key(seed: u8) -> SecretKey {
    SecretKey::from_secret_bytes(&[seed; 32]).unwrap()
}

pub(crate) fn signer(
    id: &str,
    seed: u8,
    kind: SignerKind,
    roles: &[Role],
    label: Option<&str>,
) -> Signer {
    Signer {
        id: id.to_string(),
        key: key(seed).public_key().as_str().to_string(),
        kind,
        roles: roles.to_vec(),
        label: label.map(str::to_string),
    }
}

pub(crate) fn signed<B: serde::Serialize>(
    kind: Kind,
    sequence: u64,
    list_version: u64,
    not_after: Option<DateTime<Utc>>,
    body: B,
    by: &[(&str, u8)],
) -> SignedStatement {
    let mut statement = SignedStatement::new(
        &Envelope {
            kind: kind.as_str().to_string(),
            project: PROJECT.to_string(),
            install: None,
            sequence,
            not_after,
            signer_list_version: list_version,
            body,
        }
        .to_payload()
        .unwrap(),
    );
    for (id, seed) in by {
        statement = statement.sign_with(id, &key(*seed)).unwrap();
    }
    statement
}

/// Version 1: alice's laptop (admin), CI and the recovery key.
pub(crate) fn genesis() -> SignedStatement {
    signed(
        Kind::SignerList,
        1,
        1,
        Some(now() + Duration::days(80)),
        SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![
                signer(
                    "alice-mbp",
                    1,
                    SignerKind::Device,
                    &[Role::Release, Role::Operate, Role::Admin],
                    Some("alice@acme.com"),
                ),
                signer(
                    "github-acme-app",
                    2,
                    SignerKind::Ci,
                    &[Role::Release],
                    Some("GitHub Actions (acme/app)"),
                ),
                signer("recovery", 3, SignerKind::Recovery, &[Role::Admin], None),
            ],
            deny: Vec::new(),
            recovered: false,
        },
        &[("alice-mbp", 1)],
    )
}

pub(crate) fn fingerprint() -> String {
    alien_signing::fingerprint(&genesis().open().unwrap().digest)
}

/// A stack with a function, so its protected template has a verifier.
pub(crate) fn fixture_stack() -> alien_core::Stack {
    serde_json::from_value(serde_json::json!({
        "id": "minimal",
        "resources": {
            "assets": {
                "config": {
                    "corsAllowedOrigins": [], "id": "assets", "lifecycleRules": [],
                    "publicRead": false, "type": "storage", "versioning": false
                },
                "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
            },
            "api": {
                "config": {
                    "code": {
                        "image": "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/api@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                        "type": "image"
                    },
                    "id": "api", "memoryMb": 512, "permissions": "execution",
                    "timeoutSeconds": 180, "type": "worker", "links": [], "triggers": [],
                    "environment": {}, "commandsEnabled": false
                },
                "lifecycle": "live", "dependencies": [], "remoteAccess": false
            }
        },
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .unwrap()
}

/// What the hosting service renders with.
pub(crate) fn hosting() -> SetupHostingOptions {
    serde_json::from_value(serde_json::json!({
        "registration": { "mode": "outputsFallback" },
        "managementTrust": {
            "requireExternalId": true,
            "allowedManagingRoleArns": ["arn:aws:iam::999988887777:role/vendor-manager"]
        }
    }))
    .unwrap()
}

/// Render setup template `name` of [`fixture_stack`] for the `production` channel with this
/// renderer version (or `renderer`), the way the hosting service does.
pub(crate) fn render_fixture(
    name: &'static str,
    channel: &'static str,
    renderer: Option<&'static str>,
) -> RenderedSetupTemplate {
    // A runtime of its own on its own thread: callers may already be inside one.
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let stack = fixture_stack();
                let signing = alien_core::StackSigning {
                    fingerprint: fingerprint(),
                    channel: channel.to_string(),
                };
                let cloudformation = alien_cloudformation::CfRegistry::built_in();
                let terraform = alien_terraform::TfRegistry::built_in();
                let helm = alien_helm::HelmRegistry::built_in();
                let settings = alien_cli_common::setup_templates::protected_stack_settings(
                    name, &stack, &signing,
                )
                .unwrap();
                let mut options = SetupRenderOptions::builder()
                    .signing(&signing)
                    .cloudformation_registry(&cloudformation)
                    .terraform_registry(&terraform)
                    .helm_registry(&helm)
                    .hosting(hosting())
                    .pins(crate::commands::verify_render::test_pins())
                    .build();
                if let Some(renderer) = renderer {
                    options.renderer = renderer.to_string();
                }
                render_setup_template(name, &stack, &settings, &options)
                    .await
                    .unwrap()
            })
    })
    .join()
    .unwrap()
}

/// The setup template as CloudFormation returns it for the stack.
pub(crate) fn template() -> String {
    String::from_utf8(render_fixture(CLOUDFORMATION, "production", None).bytes).unwrap()
}

fn sha256(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

/// A release of the fixture stack on `channel`, whose setup may be rendered by `min` or newer.
pub(crate) fn release_with_minimum(
    channel: &str,
    sequence: u64,
    min: &str,
    by: (&str, u8),
) -> SignedStatement {
    signed(
        Kind::Release,
        sequence,
        1,
        None,
        release_body(channel, min),
        &[by],
    )
}

pub(crate) fn release_body(channel: &str, min: &str) -> ReleaseBody {
    ReleaseBody {
        platform: "aws".to_string(),
        channel: channel.to_string(),
        workloads: alien_core::release_statement::release_body(
            alien_core::Platform::Aws,
            &fixture_stack(),
            &alien_core::release_statement::ReleaseOptions {
                channel: channel.to_string(),
                ..Default::default()
            },
        )
        .unwrap()
        .workloads,
        configurable: Default::default(),
        components: Default::default(),
        setup: BTreeMap::from([(
            CLOUDFORMATION.to_string(),
            SetupTemplate {
                allowed: render_fixture(CLOUDFORMATION, "production", None).allowed,
            },
        )]),
        setup_renderer: Some(SetupRenderer {
            min: min.to_string(),
        }),
        stack: Some(serde_json::to_value(fixture_stack()).unwrap()),
    }
}

fn release(channel: &str, sequence: u64, by: (&str, u8)) -> SignedStatement {
    release_with_minimum(channel, sequence, RENDERER_VERSION, by)
}

fn ledger(running: Option<&SignedStatement>) -> Vec<u8> {
    let mut ledger = serde_json::json!({
        "prefix": PREFIX,
        "created": true,
        "fingerprint": fingerprint(),
        "install": format!("aws:111122223333:us-east-1:{PREFIX}"),
        "signerList": { "sequence": 1, "digest": genesis().open().unwrap().digest },
    });
    let _ = running;
    ledger.to_string().into_bytes()
}

/// A protected stack that runs the CI-signed release 2026-11-04.3.
fn observed() -> Observed {
    let running = release("production", 20, ("github-acme-app", 2));
    Observed {
        stack_id: STACK_ID.to_string(),
        stack_name: PREFIX.to_string(),
        parameters: BTreeMap::from([
            ("SigningFingerprint".to_string(), fingerprint()),
            (
                "ManagingRoleArn".to_string(),
                "arn:aws:iam::999988887777:role/vendor-manager".to_string(),
            ),
            ("ManagementExternalId".to_string(), "ext-1".to_string()),
        ]),
        template: template(),
        settings: Some((
            format!(
                r#"{{"fingerprint":"{}","prefix":"{PREFIX}","channel":"production"}}"#,
                fingerprint()
            )
            .into_bytes(),
            Some(now() - Duration::days(1)),
        )),
        ledger: Some(ledger(Some(&running))),
        signer_lists: vec![genesis()],
        releases: vec![
            ("2026-11-04.3".to_string(), running),
            (
                "2026-11-04.2".to_string(),
                release("dev", 19, ("alice-mbp", 1)),
            ),
        ],
        signing_from_change_set: false,
    }
}

/// The same stack as a CREATE change set waiting for review: nothing exists yet, so the
/// signing settings and ledger are what the change set's template writes.
fn signing_from_change_set() -> Observed {
    let mut observed = observed();
    let (settings, ledger) = written_by_change_set(
        &observed.template,
        &observed.stack_name,
        &observed.parameters,
    )
    .expect("the template writes both signing parameters");
    observed.settings = Some((settings, None));
    observed.ledger = Some(ledger);
    observed.signing_from_change_set = true;
    observed
}

/// A CREATE change set has no signing parameters in SSM yet. Verify checks it before anything
/// runs: the template against the signed releases, the pinned fingerprint and the other
/// parameters, and the settings and ledger its template writes.
#[tokio::test]
async fn a_create_change_set_is_checked_before_anything_runs() {
    let pending = signing_from_change_set();
    assert_eq!(
        pending.settings.as_ref().unwrap().0,
        format!(
            r#"{{"fingerprint":"{}","prefix":"{PREFIX}","channel":"production"}}"#,
            fingerprint()
        )
        .into_bytes()
    );
    let lines = check(&pending, now()).await.unwrap();
    assert_eq!(
        marks(&lines),
        [
            ("Install", Mark::Info),
            ("Signing fingerprint", Mark::Info),
            ("Renderer", Mark::Pass),
            ("Registration", Mark::Info),
            ("Managing principals", Mark::Info),
            ("External id", Mark::Info),
            ("Template", Mark::Pass),
            ("Parameters", Mark::Pass),
            ("Managed by", Mark::Info),
            ("Signing state", Mark::Info),
            ("Trusted keys", Mark::Info),
        ]
    );

    // A fingerprint the change set overrides is what the install would pin; the vendor's
    // signer lists don't verify from it.
    let mut overridden = observed();
    overridden.parameters.insert(
        "SigningFingerprint".to_string(),
        alien_signing::fingerprint(&format!("sha256:{}", "ab".repeat(32))),
    );
    let (settings, ledger) =
        written_by_change_set(&overridden.template, PREFIX, &overridden.parameters).unwrap();
    overridden.settings = Some((settings, None));
    overridden.ledger = Some(ledger);
    overridden.signing_from_change_set = true;
    let error = check(&overridden, now()).await.expect_err("the overridden pin is refused");
    assert!(
        error.message.contains("don't verify from fingerprint"),
        "{}",
        error.message
    );

    // A template that doesn't write the settings creates an unprotected install.
    let unprotected = observed().template.replace(
        "/signed/${AWS::StackName}/fingerprint",
        "/elsewhere/${AWS::StackName}",
    );
    let error = written_by_change_set(&unprotected, PREFIX, &observed().parameters).unwrap_err();
    assert!(
        error.message.contains("is not protected"),
        "{}",
        error.message
    );

    // A value that references anything but the stack name and the parameters can't be
    // evaluated before the stack exists.
    let foreign = observed().template.replace(
        "${AWS::StackName}\",\"channel",
        "${AWS::AccountId}\",\"channel",
    );
    let error = written_by_change_set(&foreign, PREFIX, &observed().parameters).unwrap_err();
    assert!(
        error.message.contains("AWS::AccountId"),
        "{}",
        error.message
    );
}

fn find<'a>(lines: &'a [Line], label: &str) -> &'a Line {
    lines
        .iter()
        .find(|line| line.label == label)
        .unwrap_or_else(|| panic!("no {label} line in {}", render(lines)))
}

fn marks(lines: &[Line]) -> Vec<(&'static str, Mark)> {
    lines.iter().map(|line| (line.label, line.mark)).collect()
}

#[tokio::test]
async fn a_protected_stack_that_matches_its_signed_release_passes_on_one_screen() {
    let observed = observed();
    let lines = check(&observed, now()).await.unwrap();
    let display = display_fingerprint(&fingerprint());
    let digest = observed.releases[0].1.open().unwrap().digest;
    let template = sha256(&template());
    assert_eq!(
        render(&lines),
        format!(
            "Install              aws:111122223333:us-east-1:acme-prod\n\
             Signing fingerprint  {display}   (set by setup, 2026-11-04)\n\
             Renderer             alien {RENDERER_VERSION} (this build)  ✓\n\
             Registration         stack outputs only\n\
             Managing principals  only arn:aws:iam::999988887777:role/vendor-manager\n\
             External id          required\n\
             Template             rendered again from release 2026-11-04.3 · production · {template}  ✓\n\
             Parameters           fingerprint pinned · 2 customer choices allowed  ✓\n\
             Managed by           arn:aws:iam::999988887777:role/vendor-manager · with an external id\n\
             Trusted keys         alice@acme.com, GitHub Actions (acme/app), recovery  (list v1, expires 2027-01-24)\n",
        )
    );
}

#[tokio::test]
async fn a_tampered_template_fails() {
    let mut observed = observed();
    observed.template = observed
        .template
        .replace("Type: String", "Type: Number");
    assert_ne!(observed.template, template());
    let lines = check(&observed, now()).await.unwrap();
    assert_eq!(find(&lines, "Template").mark, Mark::Fail);
    assert!(
        find(&lines, "Template")
            .text
            .contains("is not what any release signed for production renders to"),
        "{}",
        find(&lines, "Template").text
    );
    // With no signed template there is no allow-list: the parameters can't be checked against a
    // release, so they aren't each reported as refused. The pinned fingerprint still is checked.
    assert_eq!(find(&lines, "Parameters").mark, Mark::Info);
    assert_eq!(
        find(&lines, "Parameters").text,
        "fingerprint pinned · the other 2 can't be checked against a release: the template belongs to none"
    );
    observed
        .parameters
        .insert("SigningFingerprint".to_string(), "SHA256:other".to_string());
    let lines = check(&observed, now()).await.unwrap();
    assert_eq!(find(&lines, "Parameters").mark, Mark::Fail);
    assert_eq!(
        find(&lines, "Parameters").text,
        "SigningFingerprint is SHA256:other, not the pinned fingerprint"
    );
}

#[tokio::test]
async fn a_parameter_that_overrides_the_pin_or_isnt_allowed_fails() {
    let mut overriding = observed();
    overriding.parameters.insert(
        "SigningFingerprint".to_string(),
        format!("SHA256:{}", "0".repeat(64)),
    );
    let lines = check(&overriding, now()).await.unwrap();
    assert_eq!(find(&lines, "Parameters").mark, Mark::Fail);
    assert!(
        find(&lines, "Parameters")
            .text
            .contains("not the pinned fingerprint"),
        "{}",
        find(&lines, "Parameters").text
    );

    let mut extra = observed();
    extra
        .parameters
        .insert("VerifierImage".to_string(), "attacker/image".to_string());
    let lines = check(&extra, now()).await.unwrap();
    assert_eq!(find(&lines, "Parameters").mark, Mark::Fail);
    assert!(
        find(&lines, "Parameters")
            .text
            .contains("VerifierImage is not a choice the signed release allows"),
        "{}",
        find(&lines, "Parameters").text
    );
}

#[tokio::test]
async fn a_template_signed_only_for_another_channel_fails() {
    let mut observed = observed();
    // The same stack and settings, signed only for the `dev` channel: the install follows
    // `production`, so the dev release isn't one it can match.
    observed.releases = vec![(
        "2026-11-04.2".to_string(),
        release("dev", 19, ("alice-mbp", 1)),
    )];
    let lines = check(&observed, now()).await.unwrap();
    assert_eq!(
        find(&lines, "Template").mark,
        Mark::Fail,
        "the dev release doesn't count"
    );
}

#[tokio::test]
async fn a_control_plane_that_signs_its_own_release_or_list_is_not_believed() {
    // A release signed by a key that isn't on the list matches the template,
    // but doesn't count.
    let mut forged_release = observed();
    forged_release.releases = vec![(
        "2026-11-04.3".to_string(),
        release("production", 20, ("github-acme-app", 9)),
    )];
    let lines = check(&forged_release, now()).await.unwrap();
    assert_eq!(find(&lines, "Template").mark, Mark::Fail);

    // A list of the control plane's own making doesn't verify from the pin.
    let mut forged_list = observed();
    let mut body: SignerListBody = genesis().open().unwrap().body().unwrap();
    body.signers.push(signer(
        "attacker",
        9,
        SignerKind::Ci,
        &[Role::Release],
        None,
    ));
    forged_list.signer_lists = vec![signed(
        Kind::SignerList,
        1,
        1,
        Some(now() + Duration::days(80)),
        body,
        &[("alice-mbp", 9)],
    )];
    let error = check(&forged_list, now()).await.unwrap_err();
    assert!(
        error.message.contains("don't verify from fingerprint"),
        "{}",
        error.message
    );
}

#[tokio::test]
async fn a_stack_without_settings_or_ledger_is_not_reported_as_protected() {
    let mut unprotected = observed();
    unprotected.settings = None;
    let error = check(&unprotected, now()).await.unwrap_err();
    assert!(
        error.message.contains("this install is not protected"),
        "{}",
        error.message
    );

    let mut no_ledger = observed();
    no_ledger.ledger = None;
    let error = check(&no_ledger, now()).await.unwrap_err();
    assert!(
        error.message.contains("/signed/acme-prod/state is missing"),
        "{}",
        error.message
    );

    // Settings copied from a neighbor name the neighbor's prefix.
    let mut neighbor = observed();
    neighbor.settings = Some((
        format!(
            r#"{{"fingerprint":"{}","prefix":"acme","channel":"production"}}"#,
            fingerprint()
        )
        .into_bytes(),
        None,
    ));
    check(&neighbor, now()).await.expect_err("settings for another prefix are refused");
}

#[tokio::test]
async fn a_release_whose_signer_was_removed_keeps_running_and_says_so() {
    let mut observed = observed();
    let mut body: SignerListBody = genesis().open().unwrap().body().unwrap();
    body.previous = Some(genesis().open().unwrap().digest);
    body.signers.retain(|signer| signer.kind != SignerKind::Ci);
    observed.signer_lists.push(signed(
        Kind::SignerList,
        2,
        1,
        Some(now() + Duration::days(85)),
        body,
        &[("alice-mbp", 1)],
    ));
    let lines = check(&observed, now()).await.unwrap();
    // The installed template came with that release: it is still the signed
    // one, and the screen says who signed it.
    let template = lines.iter().find(|line| line.label == "Template").unwrap();
    assert_eq!(template.mark, Mark::Info, "{}", template.text);
    assert!(
        template.text.contains("no longer on the list"),
        "{}",
        template.text
    );
    let keys = lines
        .iter()
        .find(|line| line.label == "Trusted keys")
        .unwrap();
    assert!(keys.text.contains("list v2"), "{}", keys.text);
}

#[test]
fn locking_keeps_the_existing_policy_and_adds_one_deny() {
    let resources = vec![
        "SigningFingerprintParameter".to_string(),
        "SigningStateParameter".to_string(),
        "Verifier".to_string(),
    ];
    let fresh: serde_json::Value =
        serde_json::from_str(&locked_policy(None, &resources).unwrap()).unwrap();
    assert_eq!(fresh["Statement"][0]["Effect"], "Allow");
    assert_eq!(
        fresh["Statement"][1]["Resource"],
        serde_json::json!([
            "LogicalResourceId/SigningFingerprintParameter",
            "LogicalResourceId/SigningStateParameter",
            "LogicalResourceId/Verifier"
        ])
    );
    let existing = r#"{"Statement":[{"Effect":"Deny","Action":"Update:Delete","Principal":"*","Resource":"LogicalResourceId/Database"},{"Effect":"Allow","Action":"Update:*","Principal":"*","Resource":"*"}]}"#;
    let merged = locked_policy(Some(existing), &resources).unwrap();
    let again = locked_policy(Some(&merged), &resources).unwrap();
    assert_eq!(merged, again, "locking twice changes nothing");
    let merged: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(merged["Statement"].as_array().unwrap().len(), 3);
    assert_eq!(
        merged["Statement"][0]["Resource"],
        "LogicalResourceId/Database"
    );
}

/// Which signing state verify checks. A change set on a stack with no settings yet (a CREATE
/// change set, or the update that protects an unprotected stack) is checked against what it
/// writes; a stack that has settings is checked against its own, whatever the change set says;
/// and without a change set, a stack with no settings is reported unprotected.
#[tokio::test]
async fn a_change_set_that_adds_protection_is_checked_against_what_it_writes() {
    let observed = observed();
    let unprotected = Signing {
        settings: None,
        ledger: None,
        from_change_set: false,
    };
    let adding = signing_to_check(
        unprotected.clone(),
        Some(&observed.template),
        PREFIX,
        &observed.parameters,
    )
    .unwrap();
    assert!(adding.from_change_set);
    assert_eq!(
        adding.settings.unwrap().0,
        format!(
            r#"{{"fingerprint":"{}","prefix":"{PREFIX}","channel":"production"}}"#,
            fingerprint()
        )
        .into_bytes()
    );
    assert_eq!(
        adding.ledger.unwrap(),
        format!(r#"{{"prefix":"{PREFIX}","created":true}}"#).into_bytes()
    );

    let installed = Signing {
        settings: observed.settings.clone(),
        ledger: observed.ledger.clone(),
        from_change_set: false,
    };
    assert_eq!(
        signing_to_check(
            installed.clone(),
            Some(&observed.template),
            PREFIX,
            &observed.parameters
        )
        .unwrap(),
        installed,
        "a protected stack's own settings are what count"
    );

    assert_eq!(
        signing_to_check(unprotected.clone(), None, PREFIX, &observed.parameters).unwrap(),
        unprotected
    );
    let mut without = observed.clone();
    without.settings = None;
    assert!(check(&without, now()).await
        .unwrap_err()
        .message
        .contains("this install is not protected"));
}

/// A signed setup update that changes the verifier can't run under the lock, and
/// ExecuteChangeSet takes no policy for the update. `--execute` lifts the lock from exactly what
/// the change set changes and puts the whole lock back after; a customer's own deny stays.
#[test]
fn executing_a_change_set_lifts_the_lock_only_from_what_it_changes() {
    let lockable: Vec<String> = [
        "SigningFingerprintParameter",
        "SigningStateParameter",
        "SigningVerifier",
        "SigningVerifierRole",
    ]
    .map(str::to_string)
    .to_vec();
    let customers = r#"{"Statement":[{"Effect":"Deny","Action":"Update:*","Principal":"*","Resource":["LogicalResourceId/Database"]},{"Effect":"Allow","Action":"Update:*","Principal":"*","Resource":"*"}]}"#;
    let locked = locked_policy(Some(customers), &lockable).unwrap();
    assert!(is_locked(&locked, &lockable));
    assert!(
        !is_locked(customers, &lockable),
        "a customer's deny of other resources isn't the lock"
    );

    let changed = ["SigningVerifier", "Database", "Queue"].map(str::to_string);
    let lifted: serde_json::Value =
        serde_json::from_str(&unlocked_for(&locked, &changed, &lockable).unwrap()).unwrap();
    assert_eq!(
        lifted["Statement"],
        serde_json::json!([
            {"Effect": "Deny", "Action": "Update:*", "Principal": "*",
             "Resource": ["LogicalResourceId/Database"]},
            {"Effect": "Allow", "Action": "Update:*", "Principal": "*", "Resource": "*"},
            {"Effect": "Deny", "Action": "Update:*", "Principal": "*", "Resource": [
                "LogicalResourceId/SigningFingerprintParameter",
                "LogicalResourceId/SigningStateParameter",
                "LogicalResourceId/SigningVerifierRole",
            ]},
        ]),
        "only the verifier is lifted; the customer's deny on Database stays"
    );

    // Locking again after the change set puts the whole lock back, as one statement.
    let relocked = locked_policy(
        Some(&unlocked_for(&lifted.to_string(), &lockable, &lockable).unwrap()),
        &lockable,
    )
    .unwrap();
    assert_eq!(relocked, locked);

    // A change set that changes nothing locked leaves the policy as it is.
    let untouched = unlocked_for(&locked, &["Queue".to_string()], &lockable).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&untouched).unwrap(),
        serde_json::from_str::<serde_json::Value>(&locked).unwrap()
    );
}

#[tokio::test]
async fn ssm_is_read_in_the_stacks_own_region_and_account_only() {
    let aws = aws_config::SdkConfig::builder()
        .region(aws_config::Region::new("eu-west-1"))
        .build();
    let error = ssm_client(&aws, STACK_ID)
        .await
        .expect_err("a stack in us-east-1 isn't read with eu-west-1 credentials");
    assert!(
        error
            .to_string()
            .contains("the stack is in us-east-1, but the credentials are for eu-west-1"),
        "{error}"
    );
    let error = ssm_client(&aws, "acme-prod")
        .await
        .expect_err("a stack name is not an ARN");
    assert!(
        error
            .to_string()
            .contains("is not a CloudFormation stack ARN"),
        "{error}"
    );
}

/// Creates `stack` from `template` (or updates it) and waits until it settles.
async fn apply_stack(
    cloudformation: &aws_sdk_cloudformation::Client,
    stack: &str,
    template: &str,
    tag: &str,
    update: bool,
) {
    use aws_sdk_cloudformation::types::{Parameter, Tag};
    let parameters = [
        ("SigningFingerprint", fingerprint()),
        ("VpcId", "vpc-1".to_string()),
        ("SubnetIds", "subnet-1,subnet-2".to_string()),
    ]
    .map(|(key, value)| {
        Parameter::builder()
            .parameter_key(key)
            .parameter_value(value)
            .build()
    });
    if update {
        cloudformation
            .update_stack()
            .stack_name(stack)
            .template_body(template)
            .set_parameters(Some(parameters.to_vec()))
            .send()
            .await
            .unwrap();
    } else {
        cloudformation
            .create_stack()
            .stack_name(stack)
            .template_body(template)
            .set_parameters(Some(parameters.to_vec()))
            .tags(Tag::builder().key(tag).value("true").build())
            .send()
            .await
            .unwrap();
    }
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let described = cloudformation
            .describe_stacks()
            .stack_name(stack)
            .send()
            .await
            .unwrap();
        let status = described.stacks()[0]
            .stack_status()
            .unwrap()
            .as_str()
            .to_string();
        if status.ends_with("_IN_PROGRESS") {
            continue;
        }
        assert!(
            status == "CREATE_COMPLETE" || status == "UPDATE_COMPLETE",
            "stack {stack} ended {status}"
        );
        return;
    }
}

/// The whole of `verify` against a real CloudFormation stack and its SSM
/// parameters, with the signed release and lists from a control plane: a
/// stack created from the signed template passes, and the same stack
/// updated to another template fails. Needs AWS credentials and
/// `TEST_RESOURCE_PREFIX`; the stack is deleted afterwards.
#[tokio::test]
#[ignore = "needs AWS credentials"]
async fn verify_reads_a_real_stack_and_catches_a_changed_template() {
    let tag = std::env::var("TEST_RESOURCE_PREFIX").expect("set TEST_RESOURCE_PREFIX");
    let stack = format!("{tag}-verify-{}", std::process::id());
    let aws = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .load()
        .await;
    let cloudformation = aws_sdk_cloudformation::Client::new(&aws);

    let control_plane = httpmock::MockServer::start_async().await;
    let signed = release("production", 20, ("github-acme-app", 2));
    control_plane
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/v1/releases");
            then.status(200).json_body(serde_json::json!({
                "items": [{ "id": "rel_live", "signedStacks": { "aws": signed } }]
            }));
        })
        .await;
    control_plane
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .path(format!("/v1/projects/{PROJECT}/signer-lists"));
            then.status(200)
                .json_body(serde_json::json!({ "items": [genesis()] }));
        })
        .await;
    let run = {
        let (aws, cloudformation, stack, url) = (
            aws.clone(),
            cloudformation.clone(),
            stack.clone(),
            control_plane.base_url(),
        );
        tokio::spawn(async move {
            apply_stack(&cloudformation, &stack, &template(), &tag, false).await;
            let observed = observe(&stack, None, &aws, &cloudformation, &url, None)
                .await
                .unwrap();
            let lines = check(&observed, Utc::now()).await.unwrap();
            let screen = render(&lines);
            println!("{screen}");
            assert!(lines.iter().all(|line| line.mark != Mark::Fail), "{screen}");
            assert!(
                screen.contains(&format!(
                    "matches release rel_live · production · {}",
                    sha256(&template())
                )),
                "{screen}"
            );
            assert!(screen.contains(&format!(":{stack}")), "{screen}");

            let changed = template().replace(
                "Resources:\n",
                "Resources:\n  Extra:\n    Type: AWS::SSM::Parameter\n    Properties:\n      Type: String\n      Value: extra\n",
            );
            apply_stack(&cloudformation, &stack, &changed, &tag, true).await;
            let observed = observe(&stack, None, &aws, &cloudformation, &url, None)
                .await
                .unwrap();
            let lines = check(&observed, Utc::now()).await.unwrap();
            let screen = render(&lines);
            println!("{screen}");
            let template_line = lines.iter().find(|line| line.label == "Template").unwrap();
            assert_eq!(template_line.mark, Mark::Fail, "{screen}");
        })
    };
    let outcome = run.await;
    cloudformation
        .delete_stack()
        .stack_name(&stack)
        .send()
        .await
        .unwrap();
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        match cloudformation
            .describe_stacks()
            .stack_name(&stack)
            .send()
            .await
        {
            Ok(described) => {
                let status = described.stacks()[0]
                    .stack_status()
                    .unwrap()
                    .as_str()
                    .to_string();
                if status == "DELETE_COMPLETE" {
                    break;
                }
                assert!(!status.ends_with("_FAILED"), "deleting {stack}: {status}");
            }
            // Gone.
            Err(_) => break,
        }
    }
    outcome.expect("the verify run passed");
}

/// A project that turned signing on after unsigned releases lists those
/// releases with `signedStacks: null`; verify reads past them to the signed
/// ones.
#[tokio::test]
async fn releases_from_before_signing_are_skipped() {
    let server = httpmock::MockServer::start_async().await;
    let statement = signed(
        Kind::Release,
        2,
        1,
        None,
        serde_json::json!({}),
        &[("alice-mbp", 1)],
    );
    let page = serde_json::json!({
        "items": [
            {"id": "rel_signed", "signedStacks": {"gcp": statement}},
            {"id": "rel_unsigned", "signedStacks": null},
        ],
        "nextCursor": null,
    });
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/v1/releases");
            then.status(200).json_body(page);
        })
        .await;
    server
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET)
                .path(format!("/v1/projects/{PROJECT}/signer-lists"));
            then.status(200)
                .json_body(serde_json::json!({ "items": [genesis()] }));
        })
        .await;

    let (lists, releases) = signed_releases(&server.base_url(), Some("token"), "gcp")
        .await
        .unwrap();

    assert_eq!(lists.len(), 1);
    assert_eq!(
        releases
            .iter()
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>(),
        vec!["rel_signed"]
    );
}

/// One changed byte of a protected template, in what matters (a widened IAM statement, another
/// verifier image, another fingerprint), is a template no signed release renders to: `verify`
/// renders it again and the bytes differ.
#[tokio::test]
async fn a_template_with_one_changed_byte_fails() {
    let original = template();
    let verifier = format!("verifier@sha256:{}", "5d".repeat(32));
    assert!(original.contains(&verifier), "the fixture has a verifier");
    let widened = original
        .replacen("sts:AssumeRole", "sts:*", 1)
        .replacen("Action: s3:GetObject", "Action: s3:*", 1);
    for (what, changed) in [
        (
            "another verifier image digest",
            original.replace(&verifier, &format!("verifier@sha256:{}", "ab".repeat(32))),
        ),
        (
            "another fingerprint",
            original.replace(&fingerprint(), &alien_signing::fingerprint(&"c".repeat(64))),
        ),
        ("a widened statement", widened),
        ("one more character", format!("{original}#")),
    ] {
        assert_ne!(changed, original, "{what} changes the template");
        let mut observed = observed();
        observed.template = changed;
        let lines = check(&observed, now()).await.unwrap();
        let template_line = find(&lines, "Template");
        assert_eq!(template_line.mark, Mark::Fail, "{what}: {}", render(&lines));
        assert!(
            template_line.text.contains("renders to"),
            "{what}: {}",
            template_line.text
        );
    }
}

/// A renderer version below the release's minimum is refused, even for a template that is
/// exactly what that older renderer made: no downgrade.
#[tokio::test]
async fn a_renderer_older_than_the_signed_minimum_is_refused() {
    let mut observed = observed();
    observed.template =
        String::from_utf8(render_fixture(CLOUDFORMATION, "production", Some("0.0.1")).bytes)
            .unwrap();
    let lines = check(&observed, now()).await.unwrap();
    let renderer = find(&lines, "Renderer");
    assert_eq!(renderer.mark, Mark::Fail, "{}", render(&lines));
    assert!(
        renderer.text.contains("0.0.1 is older than")
            && renderer.text.contains(RENDERER_VERSION),
        "{}",
        renderer.text
    );
    assert!(lines.iter().all(|line| line.label != "Template"));
}

/// A template another renderer version made is not checked with this one: `verify` names the
/// exact version to run, with a command that downloads and checks it, and does not guess.
#[tokio::test]
async fn a_template_from_another_renderer_version_names_the_version_to_run() {
    let mut observed = observed();
    let minimum = release_with_minimum("production", 20, "0.0.1", ("github-acme-app", 2));
    observed.releases = vec![("2026-11-04.3".to_string(), minimum)];
    observed.template =
        String::from_utf8(render_fixture(CLOUDFORMATION, "production", Some("0.0.2")).bytes)
            .unwrap();
    let lines = check(&observed, now()).await.unwrap();
    let renderer = find(&lines, "Renderer");
    assert_eq!(renderer.mark, Mark::Fail, "{}", render(&lines));
    assert!(
        renderer.text.contains("rendered by alien 0.0.2")
            && renderer.text.contains("gh release download v0.0.2")
            && renderer.text.contains("gh release verify-asset v0.0.2"),
        "{}",
        renderer.text
    );
    assert!(lines.iter().all(|line| line.label != "Template"));
}

/// A template that records no renderer can't be rendered again.
#[tokio::test]
async fn a_template_without_a_render_record_fails() {
    let mut observed = observed();
    let value: serde_yaml::Value = serde_yaml::from_str(&observed.template).unwrap();
    let mut value = value;
    value["Metadata"]
        .as_mapping_mut()
        .unwrap()
        .remove("Setup");
    observed.template = serde_yaml::to_string(&value).unwrap();
    let lines = check(&observed, now()).await.unwrap();
    assert_eq!(find(&lines, "Renderer").mark, Mark::Fail);
}

/// The hosting inputs a template records are shown, since a hacked control plane picks them:
/// a template rendered for another managing principal is a consistent template, and `verify`
/// shows that principal so the customer can see whom it lets in. An edit that doesn't also
/// change the record is caught by the byte comparison.
#[tokio::test]
async fn a_recorded_managing_principal_is_shown_and_an_unrecorded_edit_fails() {
    let mut observed = observed();
    observed.template = observed
        .template
        .replace("role/vendor-manager", "role/someone-else");
    let lines = check(&observed, now()).await.unwrap();
    assert!(
        find(&lines, "Managing principals")
            .text
            .contains("role/someone-else"),
        "{}",
        render(&lines)
    );
    assert_eq!(find(&lines, "External id").text, "required");

    // Only the trust policy's copy of the principal changed, not the record.
    let mut edited = self::observed();
    let template = edited.template.clone();
    let first = template.find("role/vendor-manager").unwrap();
    let second = first + 1 + template[first + 1..].find("role/vendor-manager").unwrap();
    edited.template = format!(
        "{}role/someone-else{}",
        &template[..second],
        &template[second + "role/vendor-manager".len()..]
    );
    let lines = check(&edited, now()).await.unwrap();
    assert_eq!(find(&lines, "Template").mark, Mark::Fail, "{}", render(&lines));
}
