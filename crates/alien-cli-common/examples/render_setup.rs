//! Render one setup template for a stack with explicit settings and pins, for checks against a
//! real cloud that need a template pinned to their own account.
//!
//! `cargo run -p alien-cli-common --example render_setup -- [--stub-compute-clusters] <template>
//!  <stack.json> <settings.json> <out-file> [<publisher-account> <verifier-repository@sha256:...>]`
//!
//! Compute clusters render through emitters a composed CLI registers; the built-in registries
//! have none. `--stub-compute-clusters` registers an emitter that emits nothing for them, so a
//! stack with containers still renders, with the management identity's grants for the cluster.
//! On protected GCP and Azure renders it names a placeholder machine network for the verifier.

use std::{env, fs, process::ExitCode};

use alien_cli_common::setup_templates::{
    render_setup_template, SetupHostingOptions, SetupRenderOptions,
};
use alien_cloudformation::{CfRegistry, ManagementTrustOptions, RegistrationMode};
use alien_core::{
    import::EmitContext, ComputeCluster, Platform, SetupPins, Stack, StackSettings, StackSigning,
};
use alien_helm::HelmRegistry;
use alien_terraform::{TfEmitter, TfFragment, TfRegistry};

/// Stands in for the compute-cluster emitters a composed CLI registers.
struct NoComputeCluster;

impl TfEmitter for NoComputeCluster {
    fn emit(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<TfFragment> {
        Ok(TfFragment::default())
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<hcl::Expression> {
        Ok(alien_terraform::expr::raw("true"))
    }

    fn machine_network(
        &self,
        ctx: &EmitContext<'_>,
    ) -> alien_core::Result<Option<hcl::Expression>> {
        let (subnet, group, identity) = match ctx.platform {
            Platform::Gcp => (
                "projects/${var.gcp_project}/regions/${var.gcp_region}/subnetworks/${local.resource_prefix}-machines",
                "${local.resource_prefix}-machines",
                "machines@${var.gcp_project}.iam.gserviceaccount.com",
            ),
            _ => (
                "/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}/providers/Microsoft.Network/virtualNetworks/machines/subnets/machines",
                "/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}/providers/Microsoft.Network/networkSecurityGroups/machines",
                "/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/machines",
            ),
        };
        let template = alien_terraform::expr::template;
        Ok(Some(alien_terraform::expr::jsonencode(
            alien_terraform::expr::object([
                ("subnetIds", hcl::Expression::Array(vec![template(subnet)])),
                (
                    "securityGroupIds",
                    hcl::Expression::Array(vec![template(group)]),
                ),
                ("identity", template(identity)),
            ]),
        )))
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let stub_compute_clusters = args
        .first()
        .is_some_and(|arg| arg == "--stub-compute-clusters");
    if stub_compute_clusters {
        args.remove(0);
    }
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
                gcp_verifier_image: Some(format!(
                    "gcr.example/verifier@sha256:{}",
                    "6e".repeat(32)
                )),
                azure_verifier_image: Some(format!(
                    "registry.example/verifier@sha256:{}",
                    "7f".repeat(32)
                )),
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
        private_input_custody: false,
        private_machine_api_url: None,
        fingerprint: String::new(),
        channel: String::new(),
    };
    let (cloudformation, mut terraform, helm) = (
        CfRegistry::built_in(),
        TfRegistry::built_in(),
        HelmRegistry::built_in(),
    );
    if stub_compute_clusters {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            terraform.register(ComputeCluster::RESOURCE_TYPE, platform, NoComputeCluster);
        }
    }
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
