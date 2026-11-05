//! A release records the digests of the setup templates the CLI renders, and the service that
//! hosts them renders them again. Their bytes must not depend on what the renderer finds on
//! its PATH. Its own binary: it changes PATH for the whole process.

use std::{fs, os::unix::fs::PermissionsExt};

use alien_cli_common::setup_templates::{
    render_setup_templates, RenderedSetupTemplate, SetupHostingOptions, SetupRenderOptions,
};
use alien_cloudformation::CfRegistry;
use alien_core::{SetupPins, Stack, StackSigning};
use alien_helm::HelmRegistry;
use alien_terraform::TfRegistry;
use std::collections::BTreeMap;

async fn render() -> BTreeMap<String, RenderedSetupTemplate> {
    let stack: Stack = serde_json::from_value(serde_json::json!({
        "id": "minimal",
        "resources": {
            "assets": {
                "config": {
                    "corsAllowedOrigins": [], "id": "assets", "lifecycleRules": [],
                    "publicRead": false, "type": "storage", "versioning": false
                },
                "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
            }
        },
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .expect("fixture stack parses");
    let signing = StackSigning {
        fingerprint: format!("SHA256:{}", "ab".repeat(32)),
        channel: "production".to_string(),
    };
    let hosting: SetupHostingOptions = serde_json::from_value(serde_json::json!({
        "registration": { "mode": "outputsFallback" }
    }))
    .expect("hosting options parse");
    let (cloudformation, terraform, helm) =
        (CfRegistry::built_in(), TfRegistry::built_in(), HelmRegistry::built_in());
    render_setup_templates(
        &stack,
        SetupRenderOptions::builder()
            .signing(&signing)
            .cloudformation_registry(&cloudformation)
            .terraform_registry(&terraform)
            .helm_registry(&helm)
            .hosting(hosting)
            .pins(SetupPins {
                aws_publisher: Some("111122223333".to_string()),
                aws_verifier_image: Some(format!("verifier@sha256:{}", "cd".repeat(32))),
                gcp_verifier_image: Some(format!("gcr.example/verifier@sha256:{}", "6e".repeat(32))),
                azure_verifier_image: Some(format!("registry.example/verifier@sha256:{}", "7f".repeat(32))),
                operator_digest: Some(format!("sha256:{}", "ef".repeat(32))),
            })
            .build(),
    )
    .await
    .expect("protected setup templates render")
}

#[tokio::test]
async fn protected_templates_ignore_tools_on_the_path() {
    // A `terraform` that rewrites every file it is asked to format.
    let tools = tempfile::tempdir().expect("temp dir");
    let terraform = tools.path().join("terraform");
    fs::write(
        &terraform,
        "#!/bin/sh\nfor f in $(find . -name '*.tf'); do echo '# reformatted' >> \"$f\"; done\n",
    )
    .expect("write fake terraform");
    fs::set_permissions(&terraform, fs::Permissions::from_mode(0o755)).expect("chmod");

    let original = std::env::var_os("PATH").expect("PATH is set");
    std::env::set_var("PATH", tools.path());
    let with_tool = render().await;
    std::env::set_var("PATH", "/nonexistent");
    let without_tool = render().await;
    std::env::set_var("PATH", original);

    for name in ["terraform-aws", "terraform-gcp", "terraform-azure"] {
        assert!(
            !String::from_utf8_lossy(&with_tool[name].bytes).contains("# reformatted"),
            "{name} ran the terraform on the PATH"
        );
    }
    assert_eq!(with_tool, without_tool);
}
