//! Top-level Terraform module generator.
//!
//! Splits the rendered module into one `.tf` file per Alien stack resource,
//! plus the supporting `versions.tf` / `variables.tf` / `providers.tf` /
//! `locals.tf` / `registration.tf` / `outputs.tf`. Mapping between `alien.ts`
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

mod providers;
mod readme;
mod registration;
mod variables;

#[cfg(test)]
mod tests;

use providers::{locals_body, providers_body, resource_prefix_body};
use readme::readme_md;
use registration::{
    helm_install_body, outputs_body, registration_body, terraform_input_values_expression,
};
use variables::{stack_inputs_for_terraform, validate_stack_inputs_for_terraform, variables_body};

use crate::{
    block::{attr, nested, resource_block},
    emitter::TfFragment,
    expr,
    naming::resource_labels,
    registry::TfRegistry,
    target::TerraformTarget,
};
use alien_core::{
    import::EmitContext, ownership_policy_for_resource_type, ErrorData, Network, NetworkSettings,
    RemoteStackManagement, Result, Stack, StackSettings,
};
use alien_error::{AlienError, IntoAlienError};
use hcl::{
    expr::Expression,
    structure::{Block, Body, Structure},
    Identifier,
};
use indexmap::IndexMap;
use std::collections::HashSet;

/// Generated Terraform module \u2014 one `.tf` file per Alien stack resource
/// plus the supporting framework (`versions.tf` / `variables.tf` /
/// `providers.tf` / `locals.tf` / `registration.tf` / `outputs.tf` / `README.md`).
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
    /// AWS regions supported by the environment that produced this
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
    pub release_id: Option<String>,
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
    pub release_name: String,
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
    let stack_inputs = stack_inputs_for_terraform(stack, target);
    validate_stack_inputs_for_terraform(&stack_inputs)?;

    let mut per_resource: IndexMap<String, TfFragment> = IndexMap::new();
    let mut registration_resources: Vec<Expression> = Vec::new();
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

        let registration_data = emitter.emit_import_ref(&ctx)?;
        registration_resources.push(expr::object([
            ("id", Expression::String(resource_id.to_string())),
            ("type", Expression::String(resource_type.to_string())),
            ("importData", registration_data),
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
    let needs_azure_management_inputs =
        matches!(target.cloud_platform(), alien_core::Platform::Azure) && has_remote_management;
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
            &stack_inputs,
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
            registration_resources,
            &shared_locals,
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
        "registration.tf".to_string(),
        render_body(registration_body(
            target,
            options.registration.as_ref(),
            &import_depends_on,
            terraform_input_values_expression(&stack_inputs),
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
            &stack_settings,
            options.helm_install.as_ref(),
            &stack_inputs,
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
    let required_version = if matches!(target, TerraformTarget::Eks) {
        ">= 1.9.0"
    } else {
        ">= 1.5.0"
    };
    let mut required: Vec<Structure> = vec![attr(
        "required_version",
        Expression::String(required_version.to_string()),
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
        provider_attrs.push(attr("helm", provider_decl_attr("hashicorp/helm", ">= 3.0")));
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
