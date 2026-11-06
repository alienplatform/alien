//! `verify --helm` against a protected chart installed in a disposable kind
//! cluster, and against the same release after values outside the signed
//! allow-list and a changed chart were installed over it.
//!
//! Run with a throwaway cluster:
//! `kind create cluster --name verify --kubeconfig /tmp/verify.kubeconfig`, then
//! `KUBECONFIG=/tmp/verify.kubeconfig ALIEN_VERIFY_TEST_KUBECONFIG=/tmp/verify.kubeconfig
//! cargo test -p alien-deploy-cli --lib verify_helm -- --ignored`.

use super::*;
use crate::commands::verify::tests::{fingerprint, genesis, signed, PROJECT};
use crate::commands::verify::{check_signed, render, signed_releases, verify_command, VerifyArgs};
use alien_cli_common::setup_templates::{
    render_setup_template, RenderedSetupTemplate, SetupHostingOptions, SetupRenderOptions,
};
use alien_core::{SetupPins, Stack, StackSettings, StackSigning};
use alien_signing::{Kind, ReleaseBody, SignedStatement};
use chrono::Utc;
use std::path::Path;
use std::process::Command;

const NAMESPACE: &str = "verify-helm";
const RELEASE: &str = "shop";

async fn chart() -> RenderedSetupTemplate {
    let signing = StackSigning {
        private_input_custody: false,
        private_machine_api_url: None,
        fingerprint: fingerprint(),
        channel: "production".to_string(),
    };
    let settings = StackSettings {
        signing: Some(signing.clone()),
        ..StackSettings::default()
    };
    let cloudformation = alien_cloudformation::CfRegistry::built_in();
    let terraform = alien_terraform::TfRegistry::built_in();
    let helm = alien_helm::HelmRegistry::built_in();
    let hosting: SetupHostingOptions = serde_json::from_value(
        serde_json::json!({ "registration": { "mode": "outputsFallback" } }),
    )
    .unwrap();
    let options = SetupRenderOptions::builder()
        .signing(&signing)
        .cloudformation_registry(&cloudformation)
        .terraform_registry(&terraform)
        .helm_registry(&helm)
        .hosting(hosting)
        .pins(SetupPins {
            aws_publisher: None,
            aws_verifier_image: None,
            gcp_verifier_image: None,
            azure_verifier_image: None,
            operator_digest: Some(format!("sha256:{}", "ef".repeat(32))),
        })
        .build();
    render_setup_template(
        HELM,
        &Stack::new("shop".to_string()).build(),
        &settings,
        &options,
    )
    .await
    .unwrap()
}

fn write_files(dir: &Path, files: &BTreeMap<String, String>) {
    for (path, contents) in files {
        let destination = dir.join(path);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(destination, contents).unwrap();
    }
}

fn run(program: &str, arguments: &[&str]) -> std::process::Output {
    let kubeconfig = std::env::var("ALIEN_VERIFY_TEST_KUBECONFIG")
        .expect("ALIEN_VERIFY_TEST_KUBECONFIG for a disposable cluster");
    Command::new(program)
        .args(arguments)
        .env("KUBECONFIG", kubeconfig)
        .output()
        .unwrap()
}

fn ok(program: &str, arguments: &[&str]) {
    let output = run(program, arguments);
    assert!(
        output.status.success(),
        "{program} {arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn install(chart: &Path, extra: &[&str]) -> std::process::Output {
    let mut arguments = vec![
        "upgrade",
        "--install",
        RELEASE,
        chart.to_str().unwrap(),
        "-n",
        NAMESPACE,
        "--create-namespace",
        "--set",
        "management.token=test-token",
        "--set",
        "management.defaultUrl=https://manager.example.com",
    ];
    arguments.extend_from_slice(extra);
    run("helm", &arguments)
}

fn release_statement(chart: &RenderedSetupTemplate) -> SignedStatement {
    signed(
        Kind::Release,
        40,
        1,
        None,
        ReleaseBody {
            platform: "kubernetes".to_string(),
            channel: "production".to_string(),
            workloads: Vec::new(),
            configurable: Default::default(),
            components: Default::default(),
            setup: BTreeMap::from([(HELM.to_string(), chart.setup_template())]),
            stack: None,
        },
        &[("github-acme-app", 2)],
    )
}

async fn screen(files: &BTreeMap<String, String>, url: &str) -> Vec<Line> {
    let observed = observe(RELEASE, NAMESPACE).await.unwrap();
    let (mut signed, installed, chart_line) = helm_install(&observed, files).unwrap();
    (signed.signer_lists, signed.releases) =
        signed_releases(url, None, "kubernetes").await.unwrap();
    let lines = check_signed(&signed, &installed, vec![chart_line], Utc::now()).unwrap();
    println!("{}", render(&lines));
    lines
}

fn find<'a>(lines: &'a [Line], label: &str) -> &'a Line {
    lines.iter().find(|line| line.label == label).unwrap()
}

#[tokio::test]
#[ignore = "needs a disposable cluster in ALIEN_VERIFY_TEST_KUBECONFIG"]
async fn a_protected_helm_release_verifies_and_changes_to_it_fail() {
    assert_eq!(
        std::env::var("KUBECONFIG").ok(),
        std::env::var("ALIEN_VERIFY_TEST_KUBECONFIG").ok(),
        "verify reads the cluster from KUBECONFIG; point both at the disposable cluster"
    );
    let rendered = chart().await;
    let files: BTreeMap<String, String> = serde_json::from_slice(&rendered.bytes).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let chart_dir = dir.path().join("chart");
    write_files(&chart_dir, &files);
    let _ = run(
        "kubectl",
        &["delete", "namespace", NAMESPACE, "--wait=true"],
    );
    let output = install(&chart_dir, &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let control_plane = httpmock::MockServer::start_async().await;
    control_plane
        .mock_async(|when, then| {
            when.method(httpmock::Method::GET).path("/v1/releases");
            then.status(200).json_body(serde_json::json!({
                "items": [{ "id": "rel_40", "signedStacks": { "kubernetes": release_statement(&rendered) } }]
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
    let url = control_plane.base_url();

    // The release as installed matches the signed chart.
    let lines = screen(&files, &url).await;
    let screen_text = render(&lines);
    assert!(
        lines.iter().all(|line| line.mark != Mark::Fail),
        "{screen_text}"
    );
    assert!(
        find(&lines, "Install").text.starts_with("k8s:"),
        "{screen_text}"
    );
    assert!(
        find(&lines, "Install").text.ends_with(":verify-helm/shop"),
        "{screen_text}"
    );
    assert_eq!(
        find(&lines, "Template").text,
        format!(
            "matches release rel_40 · production · {}",
            rendered.setup_template().sha256
        )
    );
    assert_eq!(find(&lines, "Values").text, "1 customer choice allowed");
    assert_eq!(
        find(&lines, "Installed chart").text,
        "revision 1 is this chart"
    );
    // And the command itself passes.
    verify_command(
        VerifyArgs {
            stack: None,
            change_set: None,
            lock: false,
            terraform: None,
            platform: None,
            prefix: None,
            project: None,
            subscription: None,
            resource_group: None,
            vault: None,
            helm: Some(RELEASE.to_string()),
            namespace: NAMESPACE.to_string(),
            chart: Some(chart_dir.clone()),
            manager_url: Some(url.clone()),
            token: None,
            token_file: None,
        },
        None,
    )
    .await
    .expect("verify passes on the installed release");

    // An upgrade that changes an allowed value, reusing the others, still passes.
    let output = install(
        &chart_dir,
        &["--reuse-values", "--set", "management.token=rotated"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = screen(&files, &url).await;
    assert!(
        lines.iter().all(|line| line.mark != Mark::Fail),
        "{}",
        render(&lines)
    );
    assert_eq!(
        find(&lines, "Installed chart").text,
        "revision 2 is this chart"
    );

    // Values outside the allow-list: the schema refuses them, and a release
    // installed with the schema skipped fails verify.
    let pinned = [
        "--reuse-values",
        "--set",
        "serviceAccounts.extra.name=attacker",
    ];
    let refused = install(&chart_dir, &pinned);
    assert!(
        !refused.status.success(),
        "the schema must refuse serviceAccounts"
    );
    let mut skipped = pinned.to_vec();
    skipped.push("--skip-schema-validation");
    let output = install(&chart_dir, &skipped);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = screen(&files, &url).await;
    assert_eq!(find(&lines, "Values").mark, Mark::Fail);
    assert!(
        find(&lines, "Values")
            .text
            .contains("serviceAccounts is set, and it is not a value the signed release allows"),
        "{}",
        render(&lines)
    );
    assert_eq!(find(&lines, "Template").mark, Mark::Pass);

    // A changed chart: checked against the signed chart it is not what Helm
    // installed; checked as itself it is not the signed chart.
    let mut tampered = files.clone();
    tampered.insert(
        "templates/extra.yaml".to_string(),
        "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: extra\ndata: {}\n".to_string(),
    );
    let tampered_dir = dir.path().join("tampered");
    write_files(&tampered_dir, &tampered);
    let output = install(&tampered_dir, &[]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = screen(&files, &url).await;
    assert_eq!(find(&lines, "Installed chart").mark, Mark::Fail);
    assert!(
        find(&lines, "Installed chart")
            .text
            .contains("templates/extra.yaml is installed but not in the chart"),
        "{}",
        render(&lines)
    );
    let lines = screen(&tampered, &url).await;
    assert_eq!(find(&lines, "Template").mark, Mark::Fail);

    ok(
        "kubectl",
        &["delete", "namespace", NAMESPACE, "--wait=true"],
    );
}
