//! Top-level Terraform module generator.
//!
//! Splits the rendered module into one `.tf` file per Alien stack resource,
//! plus the supporting `versions.tf` / `variables.tf` / `providers.tf` /
//! `locals.tf` / `import.tf` / `outputs.tf`. Mapping between `alien.ts`
//! resource ids and `.tf` files is 1:1 \u2014 reviewers find "what does the
//! `data` storage actually become" by opening `data.tf`.
//!
//! Every HCL token flows through `hcl-rs`'s formatter; output is
//! `terraform fmt`-clean (a best-effort `terraform fmt -recursive` pass at
//! the end aligns equals signs and similar polish).
//!
//! The K8s identity overlay (IRSA / Workload Identity / UAMI) is applied
//! after per-resource emitters when [`TerraformTarget::is_kubernetes`] is
//! true. See [`crate::k8s_identity`] (placeholder \u2014 lands under T9).

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::TfFragment,
    expr,
    naming::resource_labels,
    registry::TfRegistry,
    target::TerraformTarget,
};
use alien_core::{
    import::{EmitContext, CURRENT_SETUP_IMPORT_FORMAT_VERSION},
    ownership_policy_for_resource_type, ErrorData, Network, NetworkSettings, RemoteStackManagement,
    Result, Stack, StackSettings,
};
use alien_error::{AlienError, IntoAlienError};
use hcl::{
    expr::Expression,
    structure::{Block, BlockLabel, Body, Structure},
    Identifier,
};
use indexmap::IndexMap;
use std::collections::HashSet;

/// Generated Terraform module \u2014 one `.tf` file per Alien stack resource
/// plus the supporting framework (`versions.tf` / `variables.tf` /
/// `providers.tf` / `locals.tf` / `import.tf` / `outputs.tf` / `README.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFiles {
    /// Path -> contents, in render order. Iterating the module preserves
    /// the natural reading order: `versions.tf` first, per-resource files
    /// last before `README.md`.
    pub files: IndexMap<String, String>,
}

impl ModuleFiles {
    /// Iterate over the files in stable order (path, contents).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.as_str()))
    }

    /// Look up a file by relative path (e.g. `"main.tf"` / `"data.tf"`).
    pub fn get(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }
}

/// Options for Terraform module generation.
pub struct TerraformOptions<'a> {
    /// Per-`(ResourceType, Platform)` emitter dispatch. Most callers pass
    /// [`TfRegistry::built_in()`]; plugin-aware callers extend it before
    /// passing.
    pub registry: &'a TfRegistry,
    /// Human-friendly application name shown in generated review surfaces.
    pub display_name: Option<String>,
    pub stack_settings: StackSettings,
    /// Optional self-registration settings. When present, the generated module
    /// requires the configured provider and creates its registration resource
    /// after raw infrastructure is resolved.
    pub registration: Option<TerraformRegistration>,
    /// Optional Helm chart install settings. Only Kubernetes targets with
    /// self-registration can install the chart because the chart needs the
    /// manager deployment id and deployment token.
    pub helm_install: Option<TerraformHelmInstall>,
    /// AWS regions supported by the Alien environment that produced this
    /// module. Empty means no generated region validation.
    pub supported_aws_regions: Vec<String>,
}

/// Terraform provider dependency used by self-registering modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerraformRegistration {
    /// Local Terraform provider name, used in `required_providers`.
    pub provider_name: String,
    pub provider_source: String,
    pub provider_version: String,
    /// Resource suffix exposed by the provider. Combined with
    /// `provider_name` to form the full Terraform type, e.g.
    /// `<provider_name>_<resource_type>`.
    pub resource_type: String,
    pub setup_target: String,
    pub setup_fingerprint: String,
    pub setup_fingerprint_version: u32,
}

impl TerraformRegistration {
    fn provider_resource_type(&self) -> String {
        format!("{}_{}", self.provider_name, self.resource_type)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerraformHelmInstall {
    pub chart_ref: String,
}

/// Per-network-resource extra variables (e.g. existing VPC ids for AWS).
/// Computed once and threaded into `variables.tf`.
struct NetworkVariables {
    /// `(name, description, default)` entries for `variables.tf`.
    extra_string_vars: Vec<(String, String, Option<Expression>)>,
    /// `(name, description, default)` for list-of-string variables.
    extra_list_vars: Vec<(String, String, Option<Vec<String>>)>,
}

fn network_extra_variables(stack: &Stack, labels: &IndexMap<String, String>) -> NetworkVariables {
    let mut extra_string_vars = Vec::new();
    let mut extra_list_vars = Vec::new();
    for (resource_id, entry) in stack.resources() {
        let Some(network) = entry.config.downcast_ref::<Network>() else {
            continue;
        };
        let Some(label) = labels.get(resource_id) else {
            continue;
        };
        if let NetworkSettings::ByoVpcAws {
            vpc_id,
            public_subnet_ids,
            private_subnet_ids,
            security_group_ids,
        } = &network.settings
        {
            extra_string_vars.push((
                format!("{label}_vpc_id"),
                "Existing VPC ID supplied by the customer.".to_string(),
                Some(Expression::String(vpc_id.clone())),
            ));
            extra_list_vars.push((
                format!("{label}_public_subnet_ids"),
                "Existing public subnet IDs supplied by the customer.".to_string(),
                Some(public_subnet_ids.clone()),
            ));
            extra_list_vars.push((
                format!("{label}_private_subnet_ids"),
                "Existing private subnet IDs supplied by the customer.".to_string(),
                Some(private_subnet_ids.clone()),
            ));
            extra_list_vars.push((
                format!("{label}_security_group_ids"),
                "Existing security group IDs supplied by the customer.".to_string(),
                Some(security_group_ids.clone()),
            ));
        }
    }
    NetworkVariables {
        extra_string_vars,
        extra_list_vars,
    }
}

/// Generate a Terraform module for `stack` targeting `target`.
pub fn generate_terraform_module(
    stack: &Stack,
    target: TerraformTarget,
    options: TerraformOptions<'_>,
) -> Result<ModuleFiles> {
    let labels = resource_labels(stack)?;
    let platform = target.cloud_platform();
    let mut stack_settings = options.stack_settings.clone();
    if target.is_kubernetes()
        && matches!(
            target.cloud_platform(),
            alien_core::Platform::Aws | alien_core::Platform::Gcp
        )
        && stack_settings.network.is_none()
    {
        stack_settings.network = Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        });
    }

    let mut per_resource: IndexMap<String, TfFragment> = IndexMap::new();
    let mut imported_resources: Vec<Expression> = Vec::new();
    let mut shared_locals: IndexMap<String, Expression> = IndexMap::new();

    for (resource_id, resource) in stack.resources() {
        let resource_type = resource.config.resource_type();
        let ownership = ownership_policy_for_resource_type(resource_type.as_ref());
        if !ownership.should_emit_in_setup(resource.lifecycle) {
            continue;
        }

        let emitter = options.registry.require(&resource_type, platform)?;
        let ctx = EmitContext {
            stack,
            resource,
            resource_id,
            platform,
            stack_settings: &stack_settings,
            names: &labels,
        };

        let mut fragment = emitter.emit_with_registry(&ctx, options.registry)?;
        // Split per-emitter `locals` out of the per-resource file \u2014 they
        // belong in `locals.tf` so reviewers see all locals together.
        let local_contributions = std::mem::take(&mut fragment.locals);
        shared_locals.extend(local_contributions);
        per_resource.insert(resource_id.clone(), fragment);

        let import_ref = emitter.emit_import_ref(&ctx)?;
        imported_resources.push(expr::object([
            ("id", Expression::String(resource_id.to_string())),
            ("type", Expression::String(resource_type.to_string())),
            ("importData", import_ref),
        ]));
    }

    if target.is_kubernetes() {
        crate::k8s_identity::overlay_per_resource(
            stack,
            target,
            &labels,
            &mut per_resource,
            &mut shared_locals,
        )?;
    }
    if matches!(platform, alien_core::Platform::Gcp) {
        dedupe_gcp_support_resources(&mut per_resource)?;
    }
    apply_resource_dependencies(stack, &mut per_resource);
    if matches!(platform, alien_core::Platform::Azure) {
        emit_azure_setup_resource_role_definitions(&mut per_resource, stack)?;
        apply_azure_resource_group_dependency(stack, &labels, &mut per_resource);
    }
    let gcp_iam_propagation_dependencies = if matches!(platform, alien_core::Platform::Gcp) {
        gcp_iam_resource_addresses(&per_resource)
    } else {
        Vec::new()
    };
    let gcp_iam_propagation_barrier = if gcp_iam_propagation_dependencies.is_empty() {
        None
    } else {
        Some(expr::traversal(["time_sleep", "gcp_iam_propagation"]))
    };
    let mut import_depends_on: Vec<Expression> = per_resource
        .values()
        .flat_map(|fragment| fragment.resource_blocks.iter())
        .filter_map(resource_address)
        .collect();
    if let Some(barrier) = &gcp_iam_propagation_barrier {
        import_depends_on.push(barrier.clone());
    }

    let network_vars = network_extra_variables(stack, &labels);
    let include_kubernetes_provider =
        target.is_kubernetes() && has_resource_type(&per_resource, "kubernetes_manifest");
    let include_helm_provider =
        target.is_kubernetes() && options.registration.is_some() && options.helm_install.is_some();
    let include_azapi_provider = has_resource_type(&per_resource, "azapi_update_resource")
        || has_resource_type(&per_resource, "azapi_resource_action");
    let has_remote_management =
        stack_has_resource_type(stack, RemoteStackManagement::RESOURCE_TYPE);
    let needs_azure_management_inputs = has_remote_management || target == TerraformTarget::Aks;
    let deployment_name_default = options
        .display_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| stack.id())
        .to_string();

    let mut files: IndexMap<String, String> = IndexMap::new();
    files.insert(
        "versions.tf".to_string(),
        render_body(versions_body(
            target,
            options.registration.as_ref(),
            gcp_iam_propagation_barrier.is_some(),
            include_kubernetes_provider,
            include_helm_provider,
            include_azapi_provider,
        ))?,
    );
    files.insert(
        "variables.tf".to_string(),
        render_body(variables_body(
            target,
            &network_vars,
            &stack_settings,
            options.registration.as_ref(),
            options.helm_install.as_ref(),
            &deployment_name_default,
            needs_azure_management_inputs,
            &options.supported_aws_regions,
        )?)?,
    );
    files.insert(
        "providers.tf".to_string(),
        render_body(providers_body(
            target,
            include_kubernetes_provider,
            include_helm_provider,
            include_azapi_provider,
        ))?,
    );
    files.insert(
        "resource_prefix.tf".to_string(),
        render_body(resource_prefix_body())?,
    );
    files.insert(
        "locals.tf".to_string(),
        render_body(locals_body(
            target,
            &stack_settings,
            imported_resources,
            &shared_locals,
            options.registration.is_some(),
            has_remote_management,
        )?)?,
    );

    for (resource_id, fragment) in per_resource {
        let body = fragment_to_body(fragment);
        if !body_is_empty(&body) {
            let file_name = format!("{}.tf", resource_label_for_file(&labels, &resource_id));
            files.insert(file_name, render_body(body)?);
        }
    }
    if !gcp_iam_propagation_dependencies.is_empty() {
        files.insert(
            "iam_propagation.tf".to_string(),
            render_body(gcp_iam_propagation_body(&gcp_iam_propagation_dependencies))?,
        );
    }
    files.insert(
        "import.tf".to_string(),
        render_body(import_body(
            target,
            options.registration.as_ref(),
            &import_depends_on,
        ))?,
    );
    if let Some(helm_install) = options
        .helm_install
        .as_ref()
        .filter(|_| target.is_kubernetes() && options.registration.is_some())
    {
        files.insert(
            "helm.tf".to_string(),
            render_body(helm_install_body(
                options.registration.as_ref().expect("checked above"),
                helm_install,
            ))?,
        );
    }
    files.insert(
        "outputs.tf".to_string(),
        render_body(outputs_body(target, options.registration.as_ref()))?,
    );
    files.insert(
        "README.md".to_string(),
        readme_md(
            stack,
            target,
            options.registration.as_ref(),
            options.display_name.as_deref(),
        ),
    );

    let mut module = ModuleFiles { files };

    // Best-effort `terraform fmt` pass so output matches what
    // `terraform fmt -check` expects. If terraform isn't installed we ship
    // the hcl-rs output as-is \u2014 it parses identically; only attribute
    // equals-sign alignment differs.
    let _ = format_with_terraform(&mut module);

    Ok(module)
}

fn emit_azure_setup_resource_role_definitions(
    per_resource: &mut IndexMap<String, TfFragment>,
    stack: &Stack,
) -> Result<()> {
    let Some((_resource_id, fragment)) = per_resource.iter_mut().next() else {
        return Ok(());
    };

    crate::emitters::azure::helpers::emit_setup_resource_role_definitions(stack, fragment)
}

fn dedupe_gcp_support_resources(per_resource: &mut IndexMap<String, TfFragment>) -> Result<()> {
    let mut seen_custom_roles = HashSet::new();
    let mut seen_iam_member_grants = HashSet::new();
    let mut seen_resource_addresses: IndexMap<String, Block> = IndexMap::new();

    for fragment in per_resource.values_mut() {
        let mut retained = Vec::with_capacity(fragment.resource_blocks.len());
        for resource in std::mem::take(&mut fragment.resource_blocks) {
            if resource.identifier.as_str() != "resource" {
                retained.push(resource);
                continue;
            }

            let Some(provider_type) = resource.labels.first().map(|label| label.as_str()) else {
                retained.push(resource);
                continue;
            };

            let Some(label) = resource.labels.get(1).map(|label| label.as_str()) else {
                retained.push(resource);
                continue;
            };

            let address = format!("{provider_type}.{label}");
            if let Some(existing) = seen_resource_addresses.get(&address) {
                if existing != &resource {
                    return Err(AlienError::new(ErrorData::GenericError {
                        message: format!(
                            "generated conflicting Terraform resources for GCP support resource '{address}'"
                        ),
                    }));
                }
                continue;
            }
            seen_resource_addresses.insert(address, resource.clone());

            if provider_type == "google_project_iam_custom_role" {
                if seen_custom_roles.insert(label.to_string()) {
                    retained.push(resource);
                }
                continue;
            }

            if provider_type.ends_with("_iam_member") {
                let grant_key = format!(
                    "{provider_type}:{}",
                    terraform_body_identity(&resource.body)
                );
                if seen_iam_member_grants.insert(grant_key) {
                    retained.push(resource);
                }
                continue;
            }

            retained.push(resource);
        }
        fragment.resource_blocks = retained;
    }

    Ok(())
}

fn terraform_body_identity(body: &Body) -> String {
    format!("{body:?}")
}

fn fragment_to_body(fragment: TfFragment) -> Body {
    let mut structures: Vec<Structure> = Vec::new();
    for data_block in fragment.data_blocks {
        structures.push(Structure::Block(data_block));
    }
    for resource_block in fragment.resource_blocks {
        structures.push(Structure::Block(resource_block));
    }
    Body::from(structures)
}

fn apply_resource_dependencies(stack: &Stack, per_resource: &mut IndexMap<String, TfFragment>) {
    let dependency_addresses: IndexMap<String, Vec<Expression>> = per_resource
        .iter()
        .map(|(resource_id, fragment)| {
            let addresses = fragment
                .resource_blocks
                .iter()
                .filter_map(resource_address)
                .collect();
            (resource_id.clone(), addresses)
        })
        .collect();

    for (resource_id, entry) in stack.resources() {
        let Some(fragment) = per_resource.get_mut(resource_id) else {
            continue;
        };

        let mut depends_on = Vec::new();
        for dependency in &entry.dependencies {
            if dependency.id() == resource_id {
                continue;
            }
            if let Some(addresses) = dependency_addresses.get(dependency.id()) {
                for address in addresses {
                    if !depends_on.contains(address) {
                        depends_on.push(address.clone());
                    }
                }
            }
        }

        if depends_on.is_empty() {
            continue;
        }

        for resource in &mut fragment.resource_blocks {
            if !resource_inherits_stack_resource_dependencies(resource) {
                continue;
            }
            upsert_depends_on(resource, &depends_on);
        }
    }
}

fn apply_azure_resource_group_dependency(
    stack: &Stack,
    labels: &IndexMap<String, String>,
    per_resource: &mut IndexMap<String, TfFragment>,
) {
    let Some((resource_group_id, resource_group_label)) =
        stack.resources().find_map(|(resource_id, entry)| {
            if entry.config.resource_type().as_ref() != "azure_resource_group" {
                return None;
            }
            Some((resource_id.as_str(), labels.get(resource_id)?.as_str()))
        })
    else {
        return;
    };

    let dependency = expr::traversal(["azurerm_resource_group", resource_group_label]);
    for (resource_id, entry) in stack.resources() {
        if resource_id == resource_group_id
            || entry.config.resource_type().as_ref() == "service_activation"
        {
            continue;
        }
        let Some(fragment) = per_resource.get_mut(resource_id) else {
            continue;
        };
        for resource in &mut fragment.resource_blocks {
            upsert_depends_on(resource, std::slice::from_ref(&dependency));
        }
    }
}

fn resource_address(resource: &Block) -> Option<Expression> {
    if resource.identifier.as_str() != "resource" {
        return None;
    }
    let provider_type = resource.labels.first()?.as_str();
    let label = resource.labels.get(1)?.as_str();
    Some(expr::traversal([provider_type, label]))
}

fn has_resource_type(per_resource: &IndexMap<String, TfFragment>, resource_type: &str) -> bool {
    per_resource
        .values()
        .flat_map(|fragment| fragment.resource_blocks.iter())
        .any(|resource| {
            resource.identifier.as_str() == "resource"
                && resource
                    .labels
                    .first()
                    .map(|label| label.as_str() == resource_type)
                    .unwrap_or(false)
        })
}

fn stack_has_resource_type(stack: &Stack, resource_type: alien_core::ResourceType) -> bool {
    stack
        .resources()
        .any(|(_, entry)| entry.config.resource_type() == resource_type)
}

fn resource_inherits_stack_resource_dependencies(resource: &Block) -> bool {
    !is_gcp_iam_support_resource(resource)
}

fn is_gcp_iam_support_resource(resource: &Block) -> bool {
    if resource.identifier.as_str() != "resource" {
        return false;
    }

    let Some(provider_type) = resource.labels.first().map(|label| label.as_str()) else {
        return false;
    };

    provider_type == "google_project_iam_custom_role" || provider_type.ends_with("_iam_member")
}

fn gcp_iam_resource_addresses(per_resource: &IndexMap<String, TfFragment>) -> Vec<Expression> {
    per_resource
        .values()
        .flat_map(|fragment| fragment.resource_blocks.iter())
        .filter(|resource| {
            if resource.identifier.as_str() != "resource" {
                return false;
            }
            let Some(provider_type) = resource.labels.first().map(|label| label.as_str()) else {
                return false;
            };
            matches!(
                provider_type,
                "google_project_iam_member" | "google_service_account_iam_member"
            )
        })
        .filter_map(resource_address)
        .collect()
}

fn gcp_iam_propagation_body(depends_on: &[Expression]) -> Body {
    Body::from(vec![Structure::Block(resource_block(
        "time_sleep",
        "gcp_iam_propagation",
        [
            attr("create_duration", Expression::String("120s".to_string())),
            attr("depends_on", Expression::Array(depends_on.to_vec())),
        ],
    ))])
}

fn upsert_depends_on(resource: &mut Block, depends_on: &[Expression]) {
    for structure in &mut resource.body.0 {
        if let Structure::Attribute(attribute) = structure {
            if attribute.key.as_str() == "depends_on" {
                if let Expression::Array(existing) = &mut attribute.expr {
                    for dependency in depends_on {
                        if !existing.contains(dependency) {
                            existing.push(dependency.clone());
                        }
                    }
                } else {
                    attribute.expr = Expression::Array(depends_on.to_vec());
                }
                return;
            }
        }
    }

    resource
        .body
        .0
        .push(attr("depends_on", Expression::Array(depends_on.to_vec())));
}

fn body_is_empty(body: &Body) -> bool {
    body.iter().next().is_none()
}

fn resource_label_for_file<'a>(
    labels: &'a IndexMap<String, String>,
    resource_id: &'a str,
) -> &'a str {
    labels
        .get(resource_id)
        .map(String::as_str)
        .unwrap_or(resource_id)
}

/// Run `terraform fmt -recursive -write=true` over a temp dir of the
/// rendered files, then read them back. Silently no-ops if `terraform` is
/// not on PATH. Errors are silently ignored \u2014 the raw hcl-rs output is
/// itself valid HCL.
fn format_with_terraform(module: &mut ModuleFiles) -> std::io::Result<()> {
    let dir = tempfile::tempdir()?;
    for (path, contents) in module.iter() {
        if !path.ends_with(".tf") {
            continue;
        }
        std::fs::write(dir.path().join(path), contents)?;
    }

    let status = std::process::Command::new("terraform")
        .args(["fmt", "-recursive", "-write=true"])
        .current_dir(dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if !matches!(status, Ok(status) if status.success()) {
        return Ok(());
    }

    for path in module.files.keys().cloned().collect::<Vec<_>>() {
        if !path.ends_with(".tf") {
            continue;
        }
        let formatted = std::fs::read_to_string(dir.path().join(&path))?;
        module.files.insert(path, formatted);
    }
    Ok(())
}

fn render_body(body: Body) -> Result<String> {
    hcl::format::to_string(&body)
        .into_alien_error()
        .map_err(|err| {
            AlienError::new(ErrorData::TemplateSerializationFailed {
                format: "Terraform HCL".to_string(),
                reason: err.to_string(),
            })
        })
}

fn versions_body(
    target: TerraformTarget,
    registration: Option<&TerraformRegistration>,
    include_time_provider: bool,
    include_kubernetes_provider: bool,
    include_helm_provider: bool,
    include_azapi_provider: bool,
) -> Body {
    let mut required: Vec<Structure> = vec![attr(
        "required_version",
        Expression::String(">= 1.5.0".to_string()),
    )];

    let mut provider_attrs: Vec<Structure> = Vec::new();
    if matches!(target.cloud_platform(), alien_core::Platform::Aws) {
        provider_attrs.push(attr("aws", provider_decl_attr("hashicorp/aws", ">= 5.0")));
        if matches!(target, TerraformTarget::Eks) {
            provider_attrs.push(attr("tls", provider_decl_attr("hashicorp/tls", ">= 4.0")));
        }
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Gcp) {
        provider_attrs.push(attr(
            "google",
            provider_decl_attr("hashicorp/google", ">= 5.0"),
        ));
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Azure) {
        provider_attrs.push(attr(
            "azurerm",
            provider_decl_attr("hashicorp/azurerm", ">= 3.100"),
        ));
        if include_azapi_provider {
            provider_attrs.push(attr("azapi", provider_decl_attr("Azure/azapi", ">= 2.6")));
        }
    }
    if include_time_provider {
        provider_attrs.push(attr("time", provider_decl_attr("hashicorp/time", ">= 0.9")));
    }
    if include_kubernetes_provider {
        provider_attrs.push(attr(
            "kubernetes",
            provider_decl_attr("hashicorp/kubernetes", ">= 2.30"),
        ));
    }
    if include_helm_provider {
        provider_attrs.push(attr(
            "helm",
            provider_decl_attr("hashicorp/helm", ">= 2.13"),
        ));
    }
    provider_attrs.push(attr(
        "random",
        provider_decl_attr("hashicorp/random", ">= 3.6"),
    ));
    if let Some(registration) = registration {
        provider_attrs.push(attr(
            &registration.provider_name,
            provider_decl_attr(
                &registration.provider_source,
                &format!("= {}", registration.provider_version),
            ),
        ));
    }
    required.push(nested(Block {
        identifier: Identifier::sanitized("required_providers"),
        labels: vec![],
        body: Body::from(provider_attrs),
    }));

    let terraform_block = Block {
        identifier: Identifier::sanitized("terraform"),
        labels: vec![],
        body: Body::from(required),
    };

    Body::from(vec![Structure::Block(terraform_block)])
}

fn provider_decl_attr(source: &str, version: &str) -> Expression {
    expr::object([
        ("source", Expression::String(source.to_string())),
        ("version", Expression::String(version.to_string())),
    ])
}

fn variables_body(
    target: TerraformTarget,
    network_vars: &NetworkVariables,
    stack_settings: &StackSettings,
    registration: Option<&TerraformRegistration>,
    helm_install: Option<&TerraformHelmInstall>,
    deployment_name_default: &str,
    needs_azure_management_inputs: bool,
    supported_aws_regions: &[String],
) -> Result<Body> {
    let mut blocks: Vec<Structure> = Vec::new();
    let stack_settings_default = serde_json::to_string(stack_settings)
        .into_alien_error()
        .map_err(|err| {
            AlienError::new(ErrorData::JsonSerializationFailed {
                reason: format!("failed to serialize StackSettings: {err}"),
            })
        })?;

    blocks.push(nested(resource_prefix_variable_block()));

    if registration.is_some() {
        blocks.push(nested(variable_block(
            "deployment_name",
            "Human-readable application name used in setup and cloud IAM review metadata.",
            Some(Expression::String(deployment_name_default.to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "deployment_group_token",
            "Install token used when registering the resolved cloud resources.",
            None,
            true,
        )));
    } else {
        blocks.push(nested(variable_block(
            "name",
            "Human-readable application name shown in setup and cloud IAM review metadata.",
            None,
            false,
        )));
        blocks.push(nested(variable_block(
            "token",
            "Install token from the application setup page. This is the same token used by the deploy CLI --token flag.",
            None,
            true,
        )));
    }
    blocks.push(nested(variable_block(
        "manager_url",
        "Optional manager endpoint used by pull-style runtimes.",
        Some(Expression::String("".to_string())),
        false,
    )));
    blocks.push(nested(variable_block(
        "stack_settings_json",
        "Optional JSON-encoded StackSettings override supplied by deployment installers.",
        Some(Expression::String(stack_settings_default)),
        true,
    )));

    if matches!(target.cloud_platform(), alien_core::Platform::Aws) {
        blocks.push(nested(aws_region_variable_block(supported_aws_regions)));
        blocks.push(nested(variable_block(
            "managing_role_arn",
            "ARN of the manager IAM identity allowed to assume management roles.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "managing_account_id",
            "Account ID hosting the manager. Referenced by resource-side IAM policies that scope cross-account image pulls. Empty disables those grants.",
            Some(Expression::String(String::new())),
            false,
        )));
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Aws)
        && has_dynamic_aws_network_settings(stack_settings.network.as_ref())
    {
        blocks.push(nested(variable_block(
            "network_mode",
            "Choose whether this setup creates a new network, uses an existing network, or uses the default network. Values: create-new, use-existing, use-default.",
            Some(Expression::String("create-new".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "vpc_cidr",
            "CIDR for a newly-created network. Empty uses 10.42.0.0/16.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(number_variable_block(
            "availability_zones",
            "Number of availability zones to use when creating a new network.",
            Some(2),
        )));
        blocks.push(nested(variable_block(
            "vpc_id",
            "Existing VPC ID. Required when network is use-existing.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(list_variable_block(
            "public_subnet_ids",
            "Existing public subnet IDs. Required when network is use-existing.",
            Some(vec![]),
        )));
        blocks.push(nested(list_variable_block(
            "private_subnet_ids",
            "Existing private subnet IDs. Required when network is use-existing.",
            Some(vec![]),
        )));
        blocks.push(nested(list_variable_block(
            "security_group_ids",
            "Existing security group IDs. Required when network is use-existing.",
            Some(vec![]),
        )));
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Gcp) {
        blocks.push(nested(variable_block(
            "gcp_project",
            "GCP project ID.",
            None,
            false,
        )));
        blocks.push(nested(variable_block(
            "gcp_region",
            "GCP region.",
            Some(Expression::String("us-central1".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "managing_service_account_email",
            "Email of the manager's service account that may impersonate the management identity. Empty disables the binding.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(bool_variable_block(
            "gcp_manage_custom_roles",
            "Whether this module creates the GCP project custom roles it binds. Set to false when those roles are managed outside this stack.",
            Some(true),
        )));
        blocks.push(nested(variable_block(
            "gcp_custom_role_prefix",
            "Prefix used for GCP project custom role IDs when gcp_manage_custom_roles is false. Empty uses resource_prefix.",
            Some(Expression::String(String::new())),
            false,
        )));
        if has_dynamic_gcp_network_settings(stack_settings.network.as_ref()) {
            blocks.push(nested(variable_block(
                "network_mode",
                "Choose whether this setup creates a new network, uses an existing network, or uses the default network. Values: create-new, use-existing, use-default.",
                Some(Expression::String("create-new".to_string())),
                false,
            )));
            blocks.push(nested(variable_block(
                "network_cidr",
                "CIDR for a newly-created network. Empty uses 10.44.0.0/16.",
                Some(Expression::String(String::new())),
                false,
            )));
            blocks.push(nested(number_variable_block(
                "availability_zones",
                "Reserved for cross-cloud network-mode parity. GCP creates one regional subnet.",
                Some(2),
            )));
            blocks.push(nested(variable_block(
                "network_name",
                "Existing VPC network name. Required when network is use-existing.",
                Some(Expression::String(String::new())),
                false,
            )));
            blocks.push(nested(variable_block(
                "subnet_name",
                "Existing subnet name. Required when network is use-existing.",
                Some(Expression::String(String::new())),
                false,
            )));
            blocks.push(nested(variable_block(
                "network_region",
                "Existing subnet region. Empty uses gcp_region.",
                Some(Expression::String(String::new())),
                false,
            )));
        }
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Azure) {
        blocks.push(nested(variable_block(
            "azure_location",
            "Azure location.",
            Some(Expression::String("eastus".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "azure_subscription_id",
            "Azure subscription ID hosting the stack.",
            None,
            false,
        )));
        if target == TerraformTarget::Aks {
            blocks.push(nested(variable_block(
                "azure_tenant_id",
                "Azure tenant ID hosting the target AKS Kubernetes API identities.",
                None,
                false,
            )));
        }
        blocks.push(nested(variable_block(
            "azure_resource_group_name",
            "Azure resource group name.",
            None,
            false,
        )));
        if needs_azure_management_inputs {
            blocks.push(nested(variable_block(
                "azure_managing_tenant_id",
                "Azure tenant ID the manager uses for cross-tenant access.",
                None,
                false,
            )));
            blocks.push(nested(variable_block(
                "azure_oidc_issuer",
                "OIDC issuer URL for Azure Federated Identity Credential.",
                None,
                false,
            )));
            blocks.push(nested(variable_block(
                "azure_oidc_subject",
                "OIDC subject claim for Azure Federated Identity Credential.",
                None,
                false,
            )));
        }
    }
    if target.is_kubernetes() {
        blocks.push(nested(variable_block(
            "kubernetes_cluster_mode",
            "Kubernetes cluster mode. Values: create or existing.",
            Some(Expression::String("create".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "kubernetes_namespace",
            "Kubernetes namespace for runtime resources.",
            Some(Expression::String("default".to_string())),
            false,
        )));
    }
    if target.is_kubernetes() && registration.is_some() && helm_install.is_some() {
        blocks.push(nested(bool_variable_block(
            "helm_install_enabled",
            "Whether this module installs the Alien Helm chart after registering the deployment.",
            Some(true),
        )));
        blocks.push(nested(variable_block(
            "helm_release_name",
            "Helm release name used for the Alien runtime chart.",
            Some(Expression::String("alien".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "helm_chart",
            "OCI Helm chart reference to install.",
            Some(Expression::String(
                helm_install.expect("checked above").chart_ref.clone(),
            )),
            false,
        )));
    }
    if matches!(target, TerraformTarget::Eks) {
        blocks.push(nested(variable_block(
            "eks_cluster_name",
            "Existing EKS cluster name that Helm will install into.",
            Some(Expression::String(String::new())),
            false,
        )));
    }
    if matches!(target, TerraformTarget::Gke) {
        blocks.push(nested(variable_block(
            "gke_cluster_name",
            "Existing GKE cluster name that Helm will install into.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "gke_cluster_location",
            "Existing GKE cluster location (region or zone).",
            Some(Expression::String(String::new())),
            false,
        )));
    }
    if matches!(target, TerraformTarget::Aks) {
        blocks.push(nested(variable_block(
            "aks_cluster_name",
            "Existing AKS cluster name that Helm will install into.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "aks_cluster_resource_group_name",
            "Resource group containing the existing AKS cluster.",
            Some(Expression::String(String::new())),
            false,
        )));
    }

    for (name, description, default) in &network_vars.extra_string_vars {
        blocks.push(nested(variable_block(
            name,
            description,
            default.clone(),
            false,
        )));
    }
    for (name, description, default) in &network_vars.extra_list_vars {
        blocks.push(nested(list_variable_block(
            name,
            description,
            default.clone(),
        )));
    }

    Ok(Body::from(blocks))
}

fn resource_prefix_variable_block() -> Block {
    let body = vec![
        attr("type", expr::raw("string")),
        attr(
            "description",
            Expression::String(
                "Optional stable physical resource prefix. Leave empty to generate one."
                    .to_string(),
            ),
        ),
        attr("default", Expression::String(String::new())),
        nested(block(
            "validation",
            [
                attr(
                    "condition",
                    expr::raw(
                        "var.resource_prefix == \"\" || (can(regex(\"^[a-z][a-z0-9-]{1,38}[a-z0-9]$\", var.resource_prefix)) && length(regexall(\"--\", var.resource_prefix)) == 0)",
                    ),
                ),
                attr(
                    "error_message",
                    Expression::String(
                        "resource_prefix must be 3-40 characters: lowercase letters, numbers, and hyphens; start with a letter; end with a letter or number; and not contain consecutive hyphens."
                            .to_string(),
                    ),
                ),
            ],
        )),
    ];
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String("resource_prefix".to_string())],
        body: Body::from(body),
    }
}

fn aws_region_variable_block(supported_aws_regions: &[String]) -> Block {
    let default_region = supported_aws_regions
        .first()
        .cloned()
        .unwrap_or_else(|| "us-east-1".to_string());
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("string")),
        attr(
            "description",
            Expression::String("AWS region used by the AWS provider.".to_string()),
        ),
        attr("default", Expression::String(default_region)),
    ];

    if !supported_aws_regions.is_empty() {
        let supported = Expression::Array(
            supported_aws_regions
                .iter()
                .cloned()
                .map(Expression::String)
                .collect(),
        );
        let regions = supported_aws_regions.join(", ");
        body.push(nested(block(
            "validation",
            [
                attr(
                    "condition",
                    Expression::FuncCall(Box::new(
                        hcl::expr::FuncCall::builder(Identifier::sanitized("contains"))
                            .arg(supported)
                            .arg(expr::raw("var.aws_region"))
                            .build(),
                    )),
                ),
                attr(
                    "error_message",
                    Expression::String(format!(
                        "aws_region must be one of the AWS regions supported by this Alien environment: {regions}."
                    )),
                ),
            ],
        )));
    }

    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String("aws_region".to_string())],
        body: Body::from(body),
    }
}

fn list_variable_block(name: &str, description: &str, default: Option<Vec<String>>) -> Block {
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("list(string)")),
        attr("description", Expression::String(description.to_string())),
    ];
    if let Some(default) = default {
        body.push(attr(
            "default",
            Expression::Array(default.into_iter().map(Expression::String).collect()),
        ));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(body),
    }
}

fn number_variable_block(name: &str, description: &str, default: Option<i64>) -> Block {
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("number")),
        attr("description", Expression::String(description.to_string())),
    ];
    if let Some(default) = default {
        body.push(attr(
            "default",
            Expression::Number(hcl::Number::from(default)),
        ));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(body),
    }
}

fn bool_variable_block(name: &str, description: &str, default: Option<bool>) -> Block {
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("bool")),
        attr("description", Expression::String(description.to_string())),
    ];
    if let Some(default) = default {
        body.push(attr("default", Expression::Bool(default)));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(body),
    }
}

fn variable_block(
    name: &str,
    description: &str,
    default: Option<Expression>,
    sensitive: bool,
) -> Block {
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("string")),
        attr("description", Expression::String(description.to_string())),
    ];
    if let Some(default) = default {
        body.push(attr("default", default));
    }
    if sensitive {
        body.push(attr("sensitive", Expression::Bool(true)));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(body),
    }
}

fn providers_body(
    target: TerraformTarget,
    include_kubernetes_provider: bool,
    include_helm_provider: bool,
    include_azapi_provider: bool,
) -> Body {
    let mut structures: Vec<Structure> = Vec::new();
    match target.cloud_platform() {
        alien_core::Platform::Aws => {
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("provider"),
                labels: vec![BlockLabel::String("aws".to_string())],
                body: Body::from(vec![attr("region", expr::raw("var.aws_region"))]),
            }));
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("data"),
                labels: vec![
                    BlockLabel::String("aws_caller_identity".to_string()),
                    BlockLabel::String("current".to_string()),
                ],
                body: Body::default(),
            }));
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("data"),
                labels: vec![
                    BlockLabel::String("aws_region".to_string()),
                    BlockLabel::String("current".to_string()),
                ],
                body: Body::default(),
            }));
        }
        alien_core::Platform::Gcp => {
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("provider"),
                labels: vec![BlockLabel::String("google".to_string())],
                body: Body::from(vec![
                    attr("project", expr::raw("var.gcp_project")),
                    attr("region", expr::raw("var.gcp_region")),
                ]),
            }));
        }
        alien_core::Platform::Azure => {
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("provider"),
                labels: vec![BlockLabel::String("azurerm".to_string())],
                body: Body::from(vec![
                    attr(
                        "resource_provider_registrations",
                        Expression::String("none".to_string()),
                    ),
                    Structure::Block(block("features", [])),
                ]),
            }));
            if include_azapi_provider {
                structures.push(Structure::Block(Block {
                    identifier: Identifier::sanitized("provider"),
                    labels: vec![BlockLabel::String("azapi".to_string())],
                    body: Body::default(),
                }));
            }
        }
        _ => {}
    }
    if include_kubernetes_provider {
        structures.push(Structure::Block(Block {
            identifier: Identifier::sanitized("provider"),
            labels: vec![BlockLabel::String("kubernetes".to_string())],
            body: Body::from(kubernetes_provider_body(target)),
        }));
    }
    if include_helm_provider {
        structures.push(Structure::Block(Block {
            identifier: Identifier::sanitized("provider"),
            labels: vec![BlockLabel::String("helm".to_string())],
            body: Body::from(vec![nested(block(
                "kubernetes",
                kubernetes_provider_body(target),
            ))]),
        }));
    }
    Body::from(structures)
}

fn kubernetes_provider_body(target: TerraformTarget) -> Vec<Structure> {
    match target {
        TerraformTarget::Eks => vec![
            attr("host", expr::raw("data.aws_eks_cluster.target.endpoint")),
            attr(
                "cluster_ca_certificate",
                expr::raw("base64decode(data.aws_eks_cluster.target.certificate_authority[0].data)"),
            ),
            attr("token", expr::raw("data.aws_eks_cluster_auth.target.token")),
        ],
        TerraformTarget::Gke => vec![
            attr(
                "host",
                expr::raw("\"https://${data.google_container_cluster.target.endpoint}\""),
            ),
            attr(
                "cluster_ca_certificate",
                expr::raw(
                    "base64decode(data.google_container_cluster.target.master_auth[0].cluster_ca_certificate)",
                ),
            ),
            attr("token", expr::raw("data.google_client_config.current.access_token")),
        ],
        TerraformTarget::Aks => vec![
            attr(
                "host",
                expr::raw("data.azurerm_kubernetes_cluster.target.kube_config[0].host"),
            ),
            attr(
                "cluster_ca_certificate",
                expr::raw(
                    "base64decode(data.azurerm_kubernetes_cluster.target.kube_config[0].cluster_ca_certificate)",
                ),
            ),
            attr(
                "client_certificate",
                expr::raw(
                    "base64decode(data.azurerm_kubernetes_cluster.target.kube_config[0].client_certificate)",
                ),
            ),
            attr(
                "client_key",
                expr::raw("base64decode(data.azurerm_kubernetes_cluster.target.kube_config[0].client_key)"),
            ),
        ],
        _ => Vec::new(),
    }
}

fn resource_prefix_body() -> Body {
    Body::from(vec![Structure::Block(resource_block(
        "random_id",
        "resource_prefix",
        [attr(
            "byte_length",
            Expression::Number(hcl::Number::from(4)),
        )],
    ))])
}

fn locals_body(
    target: TerraformTarget,
    stack_settings: &StackSettings,
    imported_resources: Vec<Expression>,
    extra: &IndexMap<String, Expression>,
    registration: bool,
    has_remote_management: bool,
) -> Result<Body> {
    let mut body: Vec<Structure> = Vec::new();

    body.push(attr(
        "resource_prefix",
        expr::raw(
            "var.resource_prefix == \"\" ? format(\"a%s\", random_id.resource_prefix.hex) : var.resource_prefix",
        ),
    ));
    body.push(attr(
        "deployment_name",
        expr::raw(if registration {
            "var.deployment_name"
        } else {
            "var.name"
        }),
    ));
    if matches!(target.cloud_platform(), alien_core::Platform::Gcp) {
        body.push(attr(
            "gcp_custom_role_prefix",
            expr::raw(
                "substr(replace(lower(var.gcp_custom_role_prefix == \"\" ? local.resource_prefix : var.gcp_custom_role_prefix), \"-\", \"_\"), 0, 18)",
            ),
        ));
    }
    body.push(attr(
        "deployment_platform",
        Expression::String(target.deployment_platform().as_str().to_string()),
    ));
    if let Some(base_platform) = target.base_platform() {
        body.push(attr(
            "deployment_base_platform",
            Expression::String(base_platform.as_str().to_string()),
        ));
    }
    body.push(attr(
        "deployment_target",
        Expression::String(target.name().to_string()),
    ));
    body.push(attr("deployment_region", region_expression(target)));
    body.push(attr(
        "deployment_management_config",
        if has_remote_management {
            management_config_expression(target)
        } else {
            expr::raw("null")
        },
    ));
    if target.is_kubernetes() {
        if !extra.contains_key("kubernetes_exposure") {
            body.push(attr(
                "kubernetes_exposure",
                expr::object([("mode", Expression::String("disabled".to_string()))]),
            ));
        }
        body.push(attr(
            "deployment_kubernetes_settings",
            expr::raw(
                r#"merge(try(jsondecode(var.stack_settings_json).kubernetes, {}), {
  exposure = jsondecode(try(jsondecode(var.stack_settings_json).kubernetes.exposure, null) == null ? jsonencode(local.kubernetes_exposure) : jsonencode(jsondecode(var.stack_settings_json).kubernetes.exposure))
})"#,
            ),
        ));
    }
    body.push(attr(
        "deployment_settings",
        stack_settings_expression(target, stack_settings),
    ));
    body.push(attr(
        "deployment_resources",
        Expression::Array(imported_resources),
    ));

    if target.is_kubernetes() {
        for (name, value) in [
            ("kubernetes_kubeconfig", Expression::String(String::new())),
            ("kubernetes_kube_context", Expression::String(String::new())),
        ] {
            if !extra.contains_key(name) {
                body.push(attr(name, value));
            }
        }
    }

    for (name, value) in extra {
        body.push(attr(name, value.clone()));
    }
    Ok(Body::from(vec![Structure::Block(Block {
        identifier: Identifier::sanitized("locals"),
        labels: vec![],
        body: Body::from(body),
    })]))
}

fn region_expression(target: TerraformTarget) -> Expression {
    match target.cloud_platform() {
        alien_core::Platform::Aws => expr::raw("data.aws_region.current.region"),
        alien_core::Platform::Gcp => expr::raw("var.gcp_region"),
        alien_core::Platform::Azure => expr::raw("var.azure_location"),
        platform => Expression::String(platform.as_str().to_string()),
    }
}

fn management_config_expression(target: TerraformTarget) -> Expression {
    let mut object: indexmap::IndexMap<&str, Expression> = indexmap::IndexMap::new();
    object.insert(
        "platform",
        Expression::String(target.cloud_platform().as_str().to_string()),
    );
    match target.cloud_platform() {
        alien_core::Platform::Aws => {
            object.insert("managingRoleArn", expr::raw("var.managing_role_arn"));
        }
        alien_core::Platform::Gcp => {
            object.insert("projectId", expr::raw("var.gcp_project"));
            object.insert(
                "serviceAccountEmail",
                expr::raw("var.managing_service_account_email"),
            );
        }
        alien_core::Platform::Azure => {
            object.insert(
                "managingTenantId",
                expr::raw("var.azure_managing_tenant_id"),
            );
            object.insert("oidcIssuer", expr::raw("var.azure_oidc_issuer"));
            object.insert("oidcSubject", expr::raw("var.azure_oidc_subject"));
        }
        _ => {}
    }
    expr::object(object.into_iter().map(|(k, v)| (k, v)))
}

fn stack_settings_expression(
    target: TerraformTarget,
    stack_settings: &StackSettings,
) -> Expression {
    match target.cloud_platform() {
        alien_core::Platform::Aws
            if has_dynamic_aws_network_settings(stack_settings.network.as_ref()) =>
        {
            let kubernetes_settings = if target.is_kubernetes() {
                "\n  kubernetes = local.deployment_kubernetes_settings"
            } else {
                ""
            };
            return expr::raw(format!(
                r#"merge(jsondecode(var.stack_settings_json), {{
  network = jsondecode(
    var.network_mode == "create-new" ? jsonencode({{
      type              = "create"
      cidr              = var.vpc_cidr == "" ? null : var.vpc_cidr
      availabilityZones = var.availability_zones
    }}) : var.network_mode == "use-existing" ? jsonencode({{
      type             = "byo-vpc-aws"
      vpcId            = var.vpc_id
      publicSubnetIds  = var.public_subnet_ids
      privateSubnetIds = var.private_subnet_ids
      securityGroupIds = var.security_group_ids
    }}) : jsonencode({{
      type = "use-default"
    }})
  )
{kubernetes_settings}
}})"#
            ));
        }
        alien_core::Platform::Gcp
            if has_dynamic_gcp_network_settings(stack_settings.network.as_ref()) =>
        {
            let kubernetes_settings = if target.is_kubernetes() {
                "\n  kubernetes = local.deployment_kubernetes_settings"
            } else {
                ""
            };
            return expr::raw(format!(
                r#"merge(jsondecode(var.stack_settings_json), {{
  network = jsondecode(
    var.network_mode == "create-new" ? jsonencode({{
      type              = "create"
      cidr              = var.network_cidr == "" ? null : var.network_cidr
      availabilityZones = var.availability_zones
    }}) : var.network_mode == "use-existing" ? jsonencode({{
      type        = "byo-vpc-gcp"
      networkName = var.network_name
      subnetName  = var.subnet_name
      region      = var.network_region == "" ? var.gcp_region : var.network_region
    }}) : jsonencode({{
      type = "use-default"
    }})
  )
{kubernetes_settings}
}})"#
            ));
        }
        _ if target.is_kubernetes() => {
            return expr::raw(
                r#"merge(jsondecode(var.stack_settings_json), {
  kubernetes = local.deployment_kubernetes_settings
})"#,
            );
        }
        _ => return expr::raw("jsondecode(var.stack_settings_json)"),
    }
}

fn has_dynamic_aws_network_settings(network: Option<&NetworkSettings>) -> bool {
    matches!(
        network,
        Some(
            NetworkSettings::Create { .. }
                | NetworkSettings::UseDefault
                | NetworkSettings::ByoVpcAws { .. }
        )
    )
}

fn has_dynamic_gcp_network_settings(network: Option<&NetworkSettings>) -> bool {
    matches!(network, Some(NetworkSettings::Create { .. }))
}

fn import_body(
    target: TerraformTarget,
    registration: Option<&TerraformRegistration>,
    depends_on: &[Expression],
) -> Body {
    let depends_on_attr = (!depends_on.is_empty()).then(|| {
        attr(
            "depends_on",
            Expression::Array(depends_on.iter().cloned().collect()),
        )
    });
    if let Some(registration) = registration {
        let mut body = vec![
            attr(
                "deployment_group_token",
                expr::raw("var.deployment_group_token"),
            ),
            attr("name", expr::raw("local.deployment_name")),
            attr("resource_prefix", expr::raw("local.resource_prefix")),
            attr(
                "setup_target",
                Expression::String(registration.setup_target.clone()),
            ),
            attr(
                "setup_import_format_version",
                Expression::Number(hcl::Number::from(i64::from(
                    CURRENT_SETUP_IMPORT_FORMAT_VERSION,
                ))),
            ),
            attr(
                "setup_fingerprint",
                Expression::String(registration.setup_fingerprint.clone()),
            ),
            attr(
                "setup_fingerprint_version",
                Expression::Number(hcl::Number::from(i64::from(
                    registration.setup_fingerprint_version,
                ))),
            ),
            attr("platform", expr::raw("local.deployment_platform")),
            attr("region", expr::raw("local.deployment_region")),
            attr("manager_url", expr::raw("var.manager_url")),
            attr(
                "management_config",
                expr::raw("local.deployment_management_config"),
            ),
            attr("stack_settings", expr::raw("local.deployment_settings")),
            attr("resources", expr::raw("local.deployment_resources")),
        ];
        if target.is_kubernetes() {
            body.push(attr(
                "base_platform",
                expr::raw("local.deployment_base_platform"),
            ));
        }
        if let Some(depends_on_attr) = depends_on_attr {
            body.push(depends_on_attr);
        }
        return Body::from(vec![Structure::Block(resource_block(
            &registration.provider_resource_type(),
            "this",
            body,
        ))]);
    }

    let mut body = vec![attr(
        "input",
        expr::object(
            [
                ("platform", expr::raw("local.deployment_platform")),
                ("token", expr::raw("var.token")),
                ("name", expr::raw("var.name")),
                ("resource_prefix", expr::raw("local.resource_prefix")),
                (
                    "setup_target",
                    Expression::String(
                        registration
                            .map(|r| r.setup_target.clone())
                            .unwrap_or_default(),
                    ),
                ),
                (
                    "setup_import_format_version",
                    Expression::Number(hcl::Number::from(i64::from(
                        CURRENT_SETUP_IMPORT_FORMAT_VERSION,
                    ))),
                ),
                (
                    "setup_fingerprint",
                    Expression::String(
                        registration
                            .map(|r| r.setup_fingerprint.clone())
                            .unwrap_or_default(),
                    ),
                ),
                (
                    "setup_fingerprint_version",
                    Expression::Number(hcl::Number::from(i64::from(
                        registration
                            .map(|r| r.setup_fingerprint_version)
                            .unwrap_or_default(),
                    ))),
                ),
                ("manager_url", expr::raw("var.manager_url")),
                (
                    "management_config",
                    expr::raw("local.deployment_management_config"),
                ),
                ("stack_settings", expr::raw("local.deployment_settings")),
                ("resources", expr::raw("local.deployment_resources")),
            ]
            .into_iter()
            .chain(
                target
                    .is_kubernetes()
                    .then(|| ("basePlatform", expr::raw("local.deployment_base_platform"))),
            ),
        ),
    )];
    if let Some(depends_on_attr) = depends_on_attr {
        body.push(depends_on_attr);
    }

    Body::from(vec![Structure::Block(resource_block(
        "terraform_data",
        "deployment_import",
        body,
    ))])
}

fn helm_install_body(
    registration: &TerraformRegistration,
    _helm_install: &TerraformHelmInstall,
) -> Body {
    Body::from(vec![Structure::Block(resource_block(
        "helm_release",
        "alien",
        [
            attr("count", expr::raw("var.helm_install_enabled ? 1 : 0")),
            attr("name", expr::raw("var.helm_release_name")),
            attr("namespace", expr::raw("var.kubernetes_namespace")),
            attr("create_namespace", Expression::Bool(true)),
            attr("chart", expr::raw("var.helm_chart")),
            attr(
                "values",
                Expression::Array(vec![expr::raw(format!(
                    "{}.this.helm_values",
                    registration.provider_resource_type()
                ))]),
            ),
            attr(
                "depends_on",
                Expression::Array(vec![expr::raw(format!(
                    "{}.this",
                    registration.provider_resource_type()
                ))]),
            ),
        ],
    ))])
}

fn outputs_body(target: TerraformTarget, registration: Option<&TerraformRegistration>) -> Body {
    let mut outputs = vec![
        (
            "deployment_target",
            Expression::String(target.name().to_string()),
            "Terraform module target.",
        ),
        (
            "deployment_resource_prefix",
            expr::raw("local.resource_prefix"),
            "Physical resource prefix.",
        ),
        (
            "deployment_platform",
            expr::raw("local.deployment_platform"),
            "Target platform.",
        ),
        (
            "deployment_region",
            expr::raw("local.deployment_region"),
            "Target cloud region or location.",
        ),
        (
            "deployment_setup_target",
            Expression::String(
                registration
                    .map(|registration| registration.setup_target.clone())
                    .unwrap_or_default(),
            ),
            "Setup target.",
        ),
        (
            "deployment_setup_import_format_version",
            Expression::Number(hcl::Number::from(i64::from(
                CURRENT_SETUP_IMPORT_FORMAT_VERSION,
            ))),
            "Setup import payload format version.",
        ),
        (
            "deployment_setup_fingerprint",
            Expression::String(
                registration
                    .map(|registration| registration.setup_fingerprint.clone())
                    .unwrap_or_default(),
            ),
            "Setup compatibility fingerprint.",
        ),
        (
            "deployment_setup_fingerprint_version",
            Expression::Number(hcl::Number::from(i64::from(
                registration
                    .map(|registration| registration.setup_fingerprint_version)
                    .unwrap_or_default(),
            ))),
            "Setup fingerprint algorithm version.",
        ),
        (
            "deployment_management_config",
            expr::raw("jsonencode(local.deployment_management_config)"),
            "Manager import ManagementConfig JSON.",
        ),
        (
            "deployment_stack_settings",
            expr::raw("jsonencode(local.deployment_settings)"),
            "Manager import StackSettings JSON.",
        ),
        (
            "deployment_resources",
            expr::raw("jsonencode(local.deployment_resources)"),
            "Manager import resources JSON.",
        ),
    ];
    if let Some(registration) = registration {
        outputs.push((
            "deployment_id",
            expr::raw(format!(
                "{}.this.deployment_id",
                registration.provider_resource_type()
            )),
            "Manager deployment id assigned by the Terraform registration provider.",
        ));
        outputs.push((
            "deployment_token",
            expr::raw(format!(
                "{}.this.deployment_token",
                registration.provider_resource_type()
            )),
            "Deployment token assigned by the Terraform registration provider.",
        ));
    }
    if target.is_kubernetes() {
        outputs.push((
            "deployment_base_platform",
            expr::raw("local.deployment_base_platform"),
            "Base cloud platform for Kubernetes targets.",
        ));
        outputs.push((
            "kubernetes_kubeconfig",
            expr::raw("local.kubernetes_kubeconfig"),
            "Kubeconfig for managed Kubernetes clusters created by this module.",
        ));
        outputs.push((
            "kubernetes_kube_context",
            expr::raw("local.kubernetes_kube_context"),
            "Kube context for managed Kubernetes clusters created by this module.",
        ));
    }

    let blocks: Vec<Structure> = outputs
        .into_iter()
        .map(|(name, value, description)| {
            let mut body = vec![
                attr("value", value),
                attr("description", Expression::String(description.to_string())),
            ];
            if name == "deployment_stack_settings"
                || name == "deployment_token"
                || name == "kubernetes_kubeconfig"
            {
                body.push(attr("sensitive", Expression::Bool(true)));
            }

            nested(Block {
                identifier: Identifier::sanitized("output"),
                labels: vec![BlockLabel::String(name.to_string())],
                body: Body::from(body),
            })
        })
        .collect();

    Body::from(blocks)
}

fn readme_md(
    stack: &Stack,
    target: TerraformTarget,
    registration: Option<&TerraformRegistration>,
    display_name: Option<&str>,
) -> String {
    let apply_args = if registration.is_some() {
        "-var='deployment_group_token=...'".to_string()
    } else {
        format!("-var='name={}' -var='token=...'", stack.id())
    };
    let registration_note = registration
        .map(|registration| {
            format!(
                "Self-registering setup packages create `{}`; other renderers can use `deployment_management_config` / `deployment_stack_settings` / `deployment_resources` with their own registration flow.\n",
                registration.provider_resource_type()
            )
        })
        .unwrap_or_else(|| {
            "This module exposes `deployment_management_config` / `deployment_stack_settings` / `deployment_resources` for external registration flows.\n".to_string()
        });

    let display_name = display_name.unwrap_or_else(|| stack.id());
    format!(
        "# Terraform module - {}\n\n\
Target: `{}`.\n\n\
Run:\n\n\
```bash\nterraform init -backend=false\nterraform validate\nterraform apply {}\n```\n\n\
{}",
        display_name,
        target.name(),
        apply_args,
        registration_note
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{Queue, RemoteStackManagement, ResourceLifecycle, ResourceRef};

    fn block_has_depends_on(block: &Block) -> bool {
        block.body.0.iter().any(|structure| {
            matches!(
                structure,
                Structure::Attribute(attribute) if attribute.key.as_str() == "depends_on"
            )
        })
    }

    #[test]
    fn registration_uses_configured_provider_identity() {
        let registration = TerraformRegistration {
            provider_name: "example_app".to_string(),
            provider_source: "registry.example.com/acme/example-app".to_string(),
            provider_version: "1.0.2".to_string(),
            resource_type: "deployment".to_string(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "fp-test".to_string(),
            setup_fingerprint_version: 1,
        };

        let versions = render_body(versions_body(
            TerraformTarget::Aws,
            Some(&registration),
            false,
            false,
            false,
            false,
        ))
        .expect("versions render");
        assert!(versions.contains("example_app ="));
        assert!(versions.contains("registry.example.com/acme/example-app"));

        let import = render_body(import_body(TerraformTarget::Aws, Some(&registration), &[]))
            .expect("registration import render");
        assert!(import.contains("resource \"example_app_deployment\" \"this\""));

        let outputs =
            render_body(outputs_body(TerraformTarget::Aws, Some(&registration))).expect("outputs");
        assert!(outputs.contains("example_app_deployment.this.deployment_id"));
    }

    #[test]
    fn resource_prefix_validation_uses_terraform_supported_regex() {
        let variables = render_body(Body::from(vec![nested(resource_prefix_variable_block())]))
            .expect("variables render");

        assert!(variables.contains("^[a-z][a-z0-9-]{1,38}[a-z0-9]$"));
        assert!(variables.contains("length(regexall(\"--\", var.resource_prefix)) == 0"));
        assert!(!variables.contains("(?="));
    }

    #[test]
    fn stack_dependencies_skip_gcp_iam_support_resources() {
        let stack = Stack::new("test".to_string())
            .add_with_dependencies(
                Queue::new("queue".to_string()).build(),
                ResourceLifecycle::Live,
                vec![ResourceRef::new(
                    RemoteStackManagement::RESOURCE_TYPE,
                    "remote-stack-management",
                )],
            )
            .add(
                RemoteStackManagement::new("remote-stack-management".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let mut per_resource = IndexMap::new();
        per_resource.insert(
            "queue".to_string(),
            TfFragment {
                resource_blocks: vec![
                    resource_block(
                        "google_project_iam_custom_role",
                        "gcp_role_queue_heartbeat_part1",
                        [
                            attr("project", expr::raw("var.gcp_project")),
                            attr("role_id", Expression::String("role_test".to_string())),
                        ],
                    ),
                    resource_block(
                        "google_pubsub_topic",
                        "queue",
                        [attr("name", Expression::String("queue".to_string()))],
                    ),
                ],
                ..TfFragment::default()
            },
        );
        per_resource.insert(
            "remote-stack-management".to_string(),
            TfFragment {
                resource_blocks: vec![resource_block(
                    "google_project_iam_member",
                    "gcp_role_queue_heartbeat_part1_remote_stack_management_binding_0",
                    [
                        attr("project", expr::raw("var.gcp_project")),
                        attr(
                            "role",
                            expr::traversal([
                                "google_project_iam_custom_role",
                                "gcp_role_queue_heartbeat_part1",
                                "name",
                            ]),
                        ),
                    ],
                )],
                ..TfFragment::default()
            },
        );

        apply_resource_dependencies(&stack, &mut per_resource);

        let queue_fragment = per_resource.get("queue").expect("queue fragment");
        let custom_role = &queue_fragment.resource_blocks[0];
        let topic = &queue_fragment.resource_blocks[1];

        assert!(!block_has_depends_on(custom_role));
        assert!(block_has_depends_on(topic));
    }
}
