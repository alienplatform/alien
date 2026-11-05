//! Render one setup template for a stack with explicit settings and pins, for checks against a
//! real cloud that need a template pinned to their own account.
//!
//! `cargo run -p alien-cli-common --example render_setup -- <template> <stack.json>
//!  <settings.json> <out-file> [<publisher-account> <verifier-repository@sha256:...>]`

use std::{env, fs, process::ExitCode};

use alien_cli_common::setup_templates::{
    render_setup_template, SetupHostingOptions, SetupRenderOptions,
};
use alien_cloudformation::{CfRegistry, ManagementTrustOptions, RegistrationMode};
use alien_core::{SetupPins, Stack, StackSettings, StackSigning};
use alien_helm::HelmRegistry;
use alien_terraform::TfRegistry;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (name, stack, settings, out, pins) = match args.as_slice() {
        [name, stack, settings, out] => (name, stack, settings, out, SetupPins::default()),
        [name, stack, settings, out, publisher, verifier] => (
            name,
            stack,
            settings,
            out,
            SetupPins {
                aws_publisher: Some(publisher.clone()),
                aws_verifier_image: Some(verifier.clone()),
                gcp_verifier_image: Some(format!("gcr.example/verifier@sha256:{}", "6e".repeat(32))),
                azure_verifier_image: Some(format!("registry.example/verifier@sha256:{}", "7f".repeat(32))),
                operator_digest: Some(format!("sha256:{}", "ef".repeat(32))),
            },
        ),
        _ => {
            eprintln!("usage: render_setup <template> <stack.json> <settings.json> <out-file> [<publisher> <verifier-image>]");
            return ExitCode::from(2);
        }
    };
    let read =
        |path: &str| fs::read_to_string(path).unwrap_or_else(|error| panic!("{path}: {error}"));
    let stack: Stack = serde_json::from_str(&read(stack)).expect("stack JSON");
    let settings: StackSettings = serde_json::from_str(&read(settings)).expect("settings JSON");
    let unused = StackSigning {
        fingerprint: String::new(),
        channel: String::new(),
    };
    let (cloudformation, terraform, helm) = (
        CfRegistry::built_in(),
        TfRegistry::built_in(),
        HelmRegistry::built_in(),
    );
    let options = SetupRenderOptions::builder()
        .signing(&unused)
        .cloudformation_registry(&cloudformation)
        .terraform_registry(&terraform)
        .helm_registry(&helm)
        .hosting(SetupHostingOptions {
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: ManagementTrustOptions::default(),
            supported_aws_regions: Vec::new(),
            terraform_registration: None,
        })
        .pins(pins)
        .build();
    match render_setup_template(name, &stack, &settings, &options).await {
        Ok(rendered) => {
            fs::write(out, rendered.bytes).expect("write template");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error:?}");
            ExitCode::FAILURE
        }
    }
}
