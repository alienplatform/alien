//! Render every unprotected setup template for each fixture stack, the way `alien render`
//! does, into one file per template. `../run.sh` runs this against two revisions and diffs
//! the output.
//!
//! Compute clusters render through emitters a composed CLI registers; the OSS registries have
//! none. A stub that emits nothing stands in for them, so the rest of the template, including
//! the management identity's grants for the cluster, still renders.
//!
//! Usage: `setup-render-diff <fixtures-dir> <out-dir>`

use std::{env, fs, path::Path};

use alien_core::{
    import::EmitContext, AwsManagementConfig, ComputeCluster, AzureManagementConfig, DeploymentConfig, EnvironmentVariablesSnapshot,
    ExternalBindings, GcpManagementConfig, ManagementConfig, Platform, Stack, StackSettings,
    StackState,
};

/// Stands in for the compute-cluster emitters a composed CLI registers.
struct NoComputeCluster;

impl alien_cloudformation::CfEmitter for NoComputeCluster {
    fn emit_resources(
        &self,
        _ctx: &EmitContext<'_>,
    ) -> alien_core::Result<Vec<alien_cloudformation::CfResource>> {
        Ok(Vec::new())
    }

    fn emit_import_ref(
        &self,
        _ctx: &EmitContext<'_>,
    ) -> alien_core::Result<alien_cloudformation::CfExpression> {
        Ok(alien_cloudformation::CfExpression::object([(
            "stub",
            alien_cloudformation::CfExpression::from(true),
        )]))
    }
}

impl alien_terraform::TfEmitter for NoComputeCluster {
    fn emit(&self, _ctx: &EmitContext<'_>) -> alien_core::Result<alien_terraform::TfFragment> {
        Ok(alien_terraform::TfFragment::default())
    }

    fn emit_import_ref(
        &self,
        _ctx: &EmitContext<'_>,
    ) -> alien_core::Result<hcl::Expression> {
        Ok(hcl::Expression::Bool(true))
    }
}

const FORMATS: [&str; 5] = [
    "cloudformation",
    "terraform-aws",
    "terraform-gcp",
    "terraform-azure",
    "helm",
];

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let [_, fixtures, out] = args.as_slice() else {
        eprintln!("usage: setup-render-diff <fixtures-dir> <out-dir>");
        std::process::exit(2);
    };
    let mut entries: Vec<_> = fs::read_dir(fixtures)
        .expect("fixtures dir")
        .map(|entry| entry.expect("fixture entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let stack: Stack =
            serde_json::from_str(&fs::read_to_string(&path).expect("read fixture"))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
        for format in FORMATS {
            let rendered = render(&stack, format).await;
            let file = Path::new(out).join(format!("{name}.{format}.txt"));
            fs::write(&file, rendered).expect("write output");
        }
    }
}

async fn render(source: &Stack, format: &str) -> String {
    let (platform, base_platform, target) = match format {
        "cloudformation" => (Platform::Aws, None, None),
        "terraform-aws" => (Platform::Aws, None, Some(alien_terraform::TerraformTarget::Aws)),
        "terraform-gcp" => (Platform::Gcp, None, Some(alien_terraform::TerraformTarget::Gcp)),
        "terraform-azure" => (
            Platform::Azure,
            None,
            Some(alien_terraform::TerraformTarget::Azure),
        ),
        _ => (Platform::Kubernetes, None, None),
    };
    let settings = StackSettings::default();
    let mut stack = source.clone();
    let mut compute = settings.compute.clone();
    alien_core::remove_off_platform_gated_resources(&mut stack, platform, compute.as_mut());
    let source_stack = stack.clone();
    let stack = match prepare(stack, platform, base_platform, &settings).await {
        Ok(stack) => stack,
        Err(error) => return format!("PREFLIGHT ERROR: {error}\n"),
    };
    let result = match format {
        "cloudformation" => {
            let mut registry = alien_cloudformation::CfRegistry::built_in();
            registry.register(ComputeCluster::RESOURCE_TYPE, Platform::Aws, NoComputeCluster);
            alien_cloudformation::generate_cloudformation_template(
                &stack,
                alien_cloudformation::CloudFormationOptions {
                    registry: &registry,
                    source_stack: &source_stack,
                    target: alien_cloudformation::CloudFormationTarget::Aws,
                    stack_settings: settings.clone(),
                    setup_target: "aws".to_string(),
                    setup_fingerprint: "render-preview".to_string(),
                    setup_fingerprint_version: 1,
                    registration: alien_cloudformation::RegistrationMode::CustomResource {
                        lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register"
                            .to_string(),
                        callback_url: None,
                    },
                    description: None,
                    management_trust: Default::default(),
                },
            )
            .and_then(|template| alien_cloudformation::to_yaml(&template))
        }
        "helm" => {
            let registry = alien_helm::HelmRegistry::built_in();
            alien_helm::generate_helm_chart(
                &stack,
                alien_helm::HelmOptions {
                    registry: &registry,
                    stack_settings: settings.clone(),
                    chart_name: stack.id().to_string(),
                },
            )
            .map(|chart| join_files(chart.files.into_iter()))
        }
        _ => {
            let mut registry = alien_terraform::TfRegistry::built_in();
            for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
                registry.register(ComputeCluster::RESOURCE_TYPE, platform, NoComputeCluster);
            }
            alien_terraform::generate_terraform_module(
                &stack,
                target.expect("terraform target"),
                alien_terraform::TerraformOptions {
                    display_name: None,
                    registry: &registry,
                    stack_settings: settings.clone(),
                    registration: None,
                    helm_install: None,
                    supported_aws_regions: Vec::new(),
                },
            )
            .map(|module| {
                join_files(
                    module
                        .iter()
                        .map(|(path, contents)| (path.to_string(), contents.to_string())),
                )
            })
        }
    };
    match result {
        Ok(text) => text,
        Err(error) => format!("RENDER ERROR: {error}\n"),
    }
}

fn join_files(files: impl Iterator<Item = (String, String)>) -> String {
    let mut files: Vec<_> = files.collect();
    files.sort();
    files
        .into_iter()
        .map(|(path, contents)| format!("# === {path} ===\n{contents}\n"))
        .collect()
}

async fn prepare(
    stack: Stack,
    platform: Platform,
    base_platform: Option<Platform>,
    settings: &StackSettings,
) -> Result<Stack, String> {
    let runner = alien_preflights::runner::PreflightRunner::new();
    runner
        .run_template_preflights(&stack, platform)
        .await
        .map_err(|error| format!("{error} {}", serde_json::to_string(&error).unwrap_or_default()))?;
    let management_config = match base_platform.unwrap_or(platform) {
        Platform::Aws => Some(ManagementConfig::Aws(AwsManagementConfig {
            managing_role_arn: "rendered-from-template-parameter".to_string(),
        })),
        Platform::Gcp => Some(ManagementConfig::Gcp(GcpManagementConfig {
            federation_trust: None,
            service_account_email: "rendered-from-terraform-variable".to_string(),
        })),
        Platform::Azure => Some(ManagementConfig::Azure(AzureManagementConfig {
            managing_tenant_id: "rendered-from-terraform-variable".to_string(),
            oidc_issuer: "rendered-from-terraform-variable".to_string(),
            oidc_subject: "rendered-from-terraform-variable".to_string(),
        })),
        _ => None,
    };
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(settings.clone())
        .maybe_management_config(management_config)
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .maybe_base_platform(base_platform)
        .build();
    runner
        .apply_mutations(stack, &StackState::new(platform), &config)
        .await
        .map_err(|error| error.to_string())
}
