//! `verify` against what a protected stack and its control plane return,
//! and against what a tampered stack or a lying control plane return.

use super::*;
use alien_signing::{Envelope, Role, SecretKey, SetupTemplate, Signer, SignerListBody};
use chrono::Duration;

const PREFIX: &str = "acme-prod";
const STACK_ID: &str =
    "arn:aws:cloudformation:us-east-1:111122223333:stack/acme-prod/0f3a5c00-9b1e-11ef-8a4b-0e5d2f6a1b27";
const PROJECT: &str = "prj_acme";

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-11-05T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn key(seed: u8) -> SecretKey {
    SecretKey::from_secret_bytes(&[seed; 32]).unwrap()
}

fn signer(id: &str, seed: u8, kind: SignerKind, roles: &[Role], label: Option<&str>) -> Signer {
    Signer {
        id: id.to_string(),
        key: key(seed).public_key().as_str().to_string(),
        kind,
        roles: roles.to_vec(),
        label: label.map(str::to_string),
    }
}

fn signed<B: serde::Serialize>(
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
fn genesis() -> SignedStatement {
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

fn fingerprint() -> String {
    alien_signing::fingerprint(&genesis().open().unwrap().digest)
}

/// The setup template as CloudFormation returns it for the stack.
fn template() -> String {
    format!(
        r#"Description: >-
  Acme data plane. Runs only software and configuration signed with Acme's keys.
Parameters:
  SigningFingerprint:
    Type: String
    AllowedValues: ["{fingerprint}"]
    Default: "{fingerprint}"
  VpcId:
    Type: String
  SubnetIds:
    Type: CommaDelimitedList
Resources:
  SigningFingerprintParameter:
    Type: AWS::SSM::Parameter
    Properties:
      Name: !Sub "/signed/${{AWS::StackName}}/fingerprint"
      Type: String
      Value: !Sub '{{"fingerprint":"${{SigningFingerprint}}","prefix":"${{AWS::StackName}}","channel":"production"}}'
  SigningStateParameter:
    Type: AWS::SSM::Parameter
    Properties:
      Name: !Sub "/signed/${{AWS::StackName}}/state"
      Type: String
      Tier: Advanced
      Value: !Sub '{{"prefix":"${{AWS::StackName}}","created":true}}'
"#,
        fingerprint = fingerprint()
    )
}

fn sha256(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

fn release(channel: &str, sequence: u64, template: &str, by: (&str, u8)) -> SignedStatement {
    signed(
        Kind::Release,
        sequence,
        1,
        None,
        ReleaseBody {
            platform: "aws".to_string(),
            channel: channel.to_string(),
            workloads: Vec::new(),
            configurable: Default::default(),
            components: Default::default(),
            setup: BTreeMap::from([(
                CLOUDFORMATION.to_string(),
                SetupTemplate {
                    sha256: sha256(template),
                    allowed: vec![
                        "ManagementExternalId".to_string(),
                        "ManagingRoleArn".to_string(),
                        "SubnetIds".to_string(),
                        "VpcId".to_string(),
                    ],
                },
            )]),
            stack: None,
        },
        &[by],
    )
}

fn ledger(running: Option<&SignedStatement>) -> Vec<u8> {
    let mut ledger = serde_json::json!({
        "prefix": PREFIX,
        "created": true,
        "fingerprint": fingerprint(),
        "install": format!("aws:111122223333:us-east-1:{PREFIX}"),
        "signerList": { "sequence": 1, "digest": genesis().open().unwrap().digest },
    });
    if let Some(statement) = running {
        let opened = statement.open().unwrap();
        ledger["release"] =
            serde_json::json!({ "sequence": opened.sequence, "digest": opened.digest });
    }
    ledger.to_string().into_bytes()
}

/// A protected stack that runs the CI-signed release 2026-11-04.3.
fn observed() -> Observed {
    let running = release("production", 20, &template(), ("github-acme-app", 2));
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
            ("SubnetIds".to_string(), "subnet-1,subnet-2".to_string()),
            ("VpcId".to_string(), "vpc-1".to_string()),
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
                release("dev", 19, "Resources: {}\n", ("alice-mbp", 1)),
            ),
        ],
    }
}

fn marks(lines: &[Line]) -> Vec<(&'static str, Mark)> {
    lines.iter().map(|line| (line.label, line.mark)).collect()
}

#[test]
fn a_protected_stack_that_matches_its_signed_release_passes_on_one_screen() {
    let observed = observed();
    let lines = check(&observed, now()).unwrap();
    let display = display_fingerprint(&fingerprint());
    let digest = observed.releases[0].1.open().unwrap().digest;
    let template = sha256(&template());
    assert_eq!(
        render(&lines),
        format!(
            "Install              aws:111122223333:us-east-1:acme-prod\n\
             Signing fingerprint  {display}   (set by this stack, 2026-11-04)\n\
             Template             matches release 2026-11-04.3 · production · {template}  ✓\n\
             Parameters           fingerprint pinned · 4 customer choices allowed  ✓\n\
             Managed by           arn:aws:iam::999988887777:role/vendor-manager · with an external id\n\
             Running              release 2026-11-04.3 · signed by GitHub Actions (acme/app) · {digest}  ✓\n\
             Trusted keys         alice@acme.com, GitHub Actions (acme/app), recovery  (list v1, expires 2027-01-24)\n"
        )
    );
}

#[test]
fn a_tampered_template_fails() {
    let mut observed = observed();
    observed.template = observed
        .template
        .replace("Tier: Advanced", "Tier: Standard");
    let lines = check(&observed, now()).unwrap();
    assert_eq!(marks(&lines)[2], ("Template", Mark::Fail));
    assert!(lines[2]
        .text
        .contains("is not the template of any release signed for production"));
    // With no signed template to compare against, no parameter is a choice.
    assert_eq!(marks(&lines)[3], ("Parameters", Mark::Fail));
}

#[test]
fn a_parameter_that_overrides_the_pin_or_isnt_allowed_fails() {
    let mut overriding = observed();
    overriding.parameters.insert(
        "SigningFingerprint".to_string(),
        format!("SHA256:{}", "0".repeat(64)),
    );
    let lines = check(&overriding, now()).unwrap();
    assert_eq!(marks(&lines)[3], ("Parameters", Mark::Fail));
    assert!(
        lines[3].text.contains("not the pinned fingerprint"),
        "{}",
        lines[3].text
    );

    let mut extra = observed();
    extra
        .parameters
        .insert("VerifierImage".to_string(), "attacker/image".to_string());
    let lines = check(&extra, now()).unwrap();
    assert_eq!(marks(&lines)[3], ("Parameters", Mark::Fail));
    assert!(
        lines[3]
            .text
            .contains("VerifierImage is not a choice the signed release allows"),
        "{}",
        lines[3].text
    );
}

#[test]
fn a_template_signed_only_for_another_channel_fails() {
    let mut observed = observed();
    observed.template = "Resources: {}\n".to_string();
    let lines = check(&observed, now()).unwrap();
    assert_eq!(
        marks(&lines)[2],
        ("Template", Mark::Fail),
        "the dev release doesn't count"
    );
}

#[test]
fn a_control_plane_that_signs_its_own_release_or_list_is_not_believed() {
    // A release signed by a key that isn't on the list matches the template,
    // but doesn't count.
    let mut forged_release = observed();
    forged_release.releases = vec![(
        "2026-11-04.3".to_string(),
        release("production", 20, &template(), ("github-acme-app", 9)),
    )];
    let lines = check(&forged_release, now()).unwrap();
    assert_eq!(marks(&lines)[2], ("Template", Mark::Fail));

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
    let error = check(&forged_list, now()).unwrap_err();
    assert!(
        error.message.contains("don't verify from fingerprint"),
        "{}",
        error.message
    );
}

#[test]
fn a_stack_without_settings_or_ledger_is_not_reported_as_protected() {
    let mut unprotected = observed();
    unprotected.settings = None;
    let error = check(&unprotected, now()).unwrap_err();
    assert!(
        error.message.contains("this stack is not protected"),
        "{}",
        error.message
    );

    let mut no_ledger = observed();
    no_ledger.ledger = None;
    let error = check(&no_ledger, now()).unwrap_err();
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
    check(&neighbor, now()).expect_err("settings for another prefix are refused");
}

#[test]
fn a_release_whose_signer_was_removed_keeps_running_and_says_so() {
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
    let line = running(&observed);
    assert_eq!(line.mark, Mark::Info);
    assert!(line.text.contains("no longer on the list"), "{}", line.text);
    let lines = check(&observed, now()).unwrap();
    let keys = lines
        .iter()
        .find(|line| line.label == "Trusted keys")
        .unwrap();
    assert!(keys.text.contains("list v2"), "{}", keys.text);
}

fn running(observed: &Observed) -> Line {
    let lines = check(observed, now()).unwrap();
    lines
        .into_iter()
        .find(|line| line.label == "Running")
        .expect("a Running line")
}

#[test]
fn a_running_release_that_doesnt_verify_now_fails() {
    // The control plane withholds the statement the ledger names.
    let mut withheld = observed();
    withheld.releases.retain(|(id, _)| id != "2026-11-04.3");
    let line = running(&withheld);
    assert_eq!(line.mark, Mark::Fail);
    assert!(line.text.contains("didn't return it"), "{}", line.text);

    // An admin deny-listed it since.
    let mut denied = observed();
    let digest = denied.releases[0].1.open().unwrap().digest;
    let mut body: SignerListBody = genesis().open().unwrap().body().unwrap();
    body.previous = Some(genesis().open().unwrap().digest);
    body.deny = vec![digest];
    denied.signer_lists.push(signed(
        Kind::SignerList,
        2,
        1,
        Some(now() + Duration::days(85)),
        body,
        &[("alice-mbp", 1)],
    ));
    let line = running(&denied);
    assert_eq!(line.mark, Mark::Fail);
    assert!(line.text.contains("deny-listed"), "{}", line.text);

    // The ledger names a release signed for another channel.
    let mut other_channel = observed();
    let dev = other_channel.releases[1].1.clone();
    other_channel.ledger = Some(ledger(Some(&dev)));
    let line = running(&other_channel);
    assert_eq!(line.mark, Mark::Fail);
    assert!(
        line.text.contains("signed for channel dev"),
        "{}",
        line.text
    );

    // The control plane returns the running payload signed by a key that
    // isn't CI's, under CI's id.
    let mut forged = observed();
    forged.releases[0].1 = release("production", 20, &template(), ("github-acme-app", 9));
    let line = running(&forged);
    assert_eq!(line.mark, Mark::Fail);
    assert!(
        line.text
            .contains("isn't signed by release keys of signer list v1"),
        "{}",
        line.text
    );
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
    let signed = release("production", 20, &template(), ("github-acme-app", 2));
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
    let args = VerifyArgs {
        stack: stack.clone(),
        change_set: None,
        lock: false,
        manager_url: Some(control_plane.base_url()),
        token: None,
        token_file: None,
    };

    let run = {
        let (aws, cloudformation, stack, url) = (
            aws.clone(),
            cloudformation.clone(),
            stack.clone(),
            control_plane.base_url(),
        );
        tokio::spawn(async move {
            apply_stack(&cloudformation, &stack, &template(), &tag, false).await;
            let observed = observe(&args, &aws, &cloudformation, &url, None)
                .await
                .unwrap();
            let lines = check(&observed, Utc::now()).unwrap();
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
            let observed = observe(&args, &aws, &cloudformation, &url, None)
                .await
                .unwrap();
            let lines = check(&observed, Utc::now()).unwrap();
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
