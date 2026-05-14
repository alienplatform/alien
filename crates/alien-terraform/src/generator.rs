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
    import::EmitContext, ownership_policy_for_resource_type, ErrorData, Network, NetworkSettings,
    Result, Stack, StackSettings,
};
use alien_error::{AlienError, IntoAlienError};
use hcl::{
    expr::Expression,
    structure::{Block, BlockLabel, Body, Structure},
    Identifier,
};
use indexmap::IndexMap;

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
    pub stack_settings: StackSettings,
    /// Optional self-registration settings. When present, the generated module
    /// requires the configured provider and creates its registration resource
    /// after raw infrastructure is resolved.
    pub registration: Option<TerraformRegistration>,
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

/// Per-network-resource extra variables (e.g. BYO VPC ids for AWS).
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
                "BYO VPC id supplied by the customer.".to_string(),
                Some(Expression::String(vpc_id.clone())),
            ));
            extra_list_vars.push((
                format!("{label}_public_subnet_ids"),
                "BYO public subnet ids supplied by the customer.".to_string(),
                Some(public_subnet_ids.clone()),
            ));
            extra_list_vars.push((
                format!("{label}_private_subnet_ids"),
                "BYO private subnet ids supplied by the customer.".to_string(),
                Some(private_subnet_ids.clone()),
            ));
            extra_list_vars.push((
                format!("{label}_security_group_ids"),
                "BYO security group ids supplied by the customer.".to_string(),
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
    let platform = target.platform();

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
            stack_settings: &options.stack_settings,
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
    apply_resource_dependencies(stack, &mut per_resource);
    if matches!(platform, alien_core::Platform::Azure) {
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

    let mut files: IndexMap<String, String> = IndexMap::new();
    files.insert(
        "versions.tf".to_string(),
        render_body(versions_body(
            target,
            options.registration.as_ref(),
            gcp_iam_propagation_barrier.is_some(),
        ))?,
    );
    files.insert(
        "variables.tf".to_string(),
        render_body(variables_body(
            target,
            &network_vars,
            &options.stack_settings,
        )?)?,
    );
    files.insert(
        "providers.tf".to_string(),
        render_body(providers_body(target))?,
    );
    files.insert(
        "locals.tf".to_string(),
        render_body(locals_body(
            target,
            &options.stack_settings,
            imported_resources,
            &shared_locals,
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
            options.registration.as_ref(),
            &import_depends_on,
        ))?,
    );
    files.insert(
        "outputs.tf".to_string(),
        render_body(outputs_body(target, options.registration.as_ref()))?,
    );
    files.insert(
        "README.md".to_string(),
        readme_md(stack, target, options.registration.as_ref()),
    );

    let mut module = ModuleFiles { files };

    // Best-effort `terraform fmt` pass so output matches what
    // `terraform fmt -check` expects. If terraform isn't installed we ship
    // the hcl-rs output as-is \u2014 it parses identically; only attribute
    // equals-sign alignment differs.
    let _ = format_with_terraform(&mut module);

    Ok(module)
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
                "google_project_iam_custom_role"
                    | "google_project_iam_member"
                    | "google_service_account_iam_member"
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
) -> Body {
    let mut required: Vec<Structure> = vec![attr(
        "required_version",
        Expression::String(">= 1.5.0".to_string()),
    )];

    let mut provider_attrs: Vec<Structure> = Vec::new();
    if matches!(target.platform(), alien_core::Platform::Aws) {
        provider_attrs.push(attr("aws", provider_decl_attr("hashicorp/aws", ">= 5.0")));
    }
    if matches!(target.platform(), alien_core::Platform::Gcp) {
        provider_attrs.push(attr(
            "google",
            provider_decl_attr("hashicorp/google", ">= 5.0"),
        ));
        if include_time_provider {
            provider_attrs.push(attr("time", provider_decl_attr("hashicorp/time", ">= 0.9")));
        }
    }
    if matches!(target.platform(), alien_core::Platform::Azure) {
        provider_attrs.push(attr(
            "azurerm",
            provider_decl_attr("hashicorp/azurerm", ">= 3.100"),
        ));
    }
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
) -> Result<Body> {
    let mut blocks: Vec<Structure> = Vec::new();
    let stack_settings_json = serde_json::to_string(stack_settings)
        .into_alien_error()
        .map_err(|err| {
            AlienError::new(ErrorData::JsonSerializationFailed {
                reason: format!("failed to serialize StackSettings: {err}"),
            })
        })?;
    blocks.push(nested(variable_block(
        "stack_name",
        "Stable physical-name prefix for resources created by this module.",
        None,
        false,
    )));
    blocks.push(nested(variable_block(
        "deployment_name",
        "Deployment name used when registering the resolved stack import. Defaults to stack_name.",
        Some(Expression::String("".to_string())),
        false,
    )));
    blocks.push(nested(variable_block(
        "deployment_group_token",
        "Deployment group token used when registering the resolved stack import.",
        None,
        true,
    )));
    blocks.push(nested(variable_block(
        "manager_url",
        "Optional manager endpoint used by pull-style runtimes.",
        Some(Expression::String("".to_string())),
        false,
    )));
    blocks.push(nested(variable_block(
        "stack_settings_json",
        "Optional JSON-encoded StackSettings override supplied by deployment installers.",
        Some(Expression::String(stack_settings_json)),
        true,
    )));

    if matches!(target.platform(), alien_core::Platform::Aws) {
        blocks.push(nested(variable_block(
            "aws_region",
            "AWS region used by the AWS provider.",
            Some(Expression::String("us-east-1".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "managing_role_arn",
            "ARN of the manager IAM identity allowed to assume management roles.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "managing_account_id",
            "AWS account ID hosting the manager. Referenced by stack-side IAM policies that scope cross-account ECR pulls. Empty disables those grants.",
            Some(Expression::String(String::new())),
            false,
        )));
    }
    if matches!(target.platform(), alien_core::Platform::Gcp) {
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
            "gcp_use_existing_custom_roles",
            "Use pre-created project custom roles named after Alien permission sets instead of creating per-stack custom roles.",
            Some(false),
        )));
    }
    if matches!(target.platform(), alien_core::Platform::Azure) {
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
        blocks.push(nested(variable_block(
            "azure_resource_group_name",
            "Azure resource group name.",
            None,
            false,
        )));
        blocks.push(nested(variable_block(
            "azure_managing_tenant_id",
            "Azure tenant ID the manager uses for cross-tenant access.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "azure_management_principal_id",
            "Optional service-principal object ID for local development fallback role assignment.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "azure_oidc_issuer",
            "OIDC issuer URL for Azure Federated Identity Credential. Empty disables FIC creation.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "azure_oidc_subject",
            "OIDC subject claim for Azure Federated Identity Credential. Empty disables FIC creation.",
            Some(Expression::String(String::new())),
            false,
        )));
    }
    if target.is_kubernetes() {
        blocks.push(nested(variable_block(
            "kubernetes_namespace",
            "Kubernetes namespace for runtime resources.",
            Some(Expression::String("default".to_string())),
            false,
        )));
        if matches!(target, TerraformTarget::Aks) {
            blocks.push(nested(variable_block(
                "aks_oidc_issuer_url",
                "OIDC issuer URL of the AKS cluster (read from `az aks show` and supplied by the customer).",
                Some(Expression::String(String::new())),
                false,
            )));
        }
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

fn providers_body(target: TerraformTarget) -> Body {
    let mut structures: Vec<Structure> = Vec::new();
    match target.platform() {
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
        }
        _ => {}
    }
    Body::from(structures)
}

fn locals_body(
    target: TerraformTarget,
    _stack_settings: &StackSettings,
    imported_resources: Vec<Expression>,
    extra: &IndexMap<String, Expression>,
) -> Result<Body> {
    let mut body: Vec<Structure> = Vec::new();

    body.push(attr(
        "deployment_platform",
        Expression::String(target.platform().as_str().to_string()),
    ));
    body.push(attr(
        "deployment_target",
        Expression::String(target.name().to_string()),
    ));
    body.push(attr("deployment_region", region_expression(target)));
    body.push(attr(
        "deployment_management_config",
        management_config_expression(target),
    ));
    body.push(attr(
        "deployment_stack_settings",
        expr::raw("jsondecode(var.stack_settings_json)"),
    ));
    body.push(attr(
        "deployment_resources",
        Expression::Array(imported_resources),
    ));

    for (name, value) in extra {
        body.push(attr(name, value.clone()));
    }
    if target.is_kubernetes() {
        body.push(attr(
            "helm_values",
            expr::object([
                ("serviceAccounts", expr::raw("local.helm_service_accounts")),
                (
                    "stackSettings",
                    expr::raw("local.deployment_stack_settings"),
                ),
            ]),
        ));
    }

    Ok(Body::from(vec![Structure::Block(Block {
        identifier: Identifier::sanitized("locals"),
        labels: vec![],
        body: Body::from(body),
    })]))
}

fn region_expression(target: TerraformTarget) -> Expression {
    match target.platform() {
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
        Expression::String(target.platform().as_str().to_string()),
    );
    match target.platform() {
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
            object.insert(
                "managementPrincipalId",
                expr::raw(
                    "var.azure_management_principal_id == \"\" ? null : var.azure_management_principal_id",
                ),
            );
            object.insert(
                "oidcIssuer",
                expr::raw("var.azure_oidc_issuer == \"\" ? null : var.azure_oidc_issuer"),
            );
            object.insert(
                "oidcSubject",
                expr::raw("var.azure_oidc_subject == \"\" ? null : var.azure_oidc_subject"),
            );
        }
        _ => {}
    }
    expr::object(object.into_iter().map(|(k, v)| (k, v)))
}

fn import_body(registration: Option<&TerraformRegistration>, depends_on: &[Expression]) -> Body {
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
            attr(
                "name",
                expr::raw("var.deployment_name == \"\" ? var.stack_name : var.deployment_name"),
            ),
            attr("stack_prefix", expr::raw("var.stack_name")),
            attr(
                "setup_target",
                Expression::String(registration.setup_target.clone()),
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
            attr(
                "stack_settings",
                expr::raw("local.deployment_stack_settings"),
            ),
            attr("resources", expr::raw("local.deployment_resources")),
        ];
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
        expr::object([
            ("platform", expr::raw("local.deployment_platform")),
            (
                "deployment_group_token",
                expr::raw("var.deployment_group_token"),
            ),
            (
                "deployment_name",
                expr::raw("var.deployment_name == \"\" ? var.stack_name : var.deployment_name"),
            ),
            ("stack_prefix", expr::raw("var.stack_name")),
            (
                "setup_target",
                Expression::String(registration.map(|r| r.setup_target.clone()).unwrap_or_default()),
            ),
            (
                "setup_fingerprint",
                Expression::String(registration.map(|r| r.setup_fingerprint.clone()).unwrap_or_default()),
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
            (
                "stack_settings",
                expr::raw("local.deployment_stack_settings"),
            ),
            ("resources", expr::raw("local.deployment_resources")),
        ]),
    )];
    if let Some(depends_on_attr) = depends_on_attr {
        body.push(depends_on_attr);
    }

    Body::from(vec![Structure::Block(resource_block(
        "terraform_data",
        "deployment_stack_import",
        body,
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
            "deployment_stack_prefix",
            expr::raw("var.stack_name"),
            "Physical stack prefix.",
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
            expr::raw("jsonencode(local.deployment_stack_settings)"),
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
    }
    if target.is_kubernetes() {
        outputs.push((
            "helm_values",
            expr::raw("jsonencode(local.helm_values)"),
            "Helm values JSON for the manager-fetch Kubernetes install.",
        ));
    }

    let blocks: Vec<Structure> = outputs
        .into_iter()
        .map(|(name, value, description)| {
            let mut body = vec![
                attr("value", value),
                attr("description", Expression::String(description.to_string())),
            ];
            if name == "deployment_stack_settings" || name == "helm_values" {
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
) -> String {
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

    format!(
        "# Terraform module - {}\n\n\
Target: `{}`.\n\n\
Run:\n\n\
```bash\nterraform init -backend=false\nterraform validate\nterraform apply -var='stack_name={}' -var='deployment_group_token=...'\n```\n\n\
{}",
        stack.id(),
        target.name(),
        stack.id(),
        registration_note
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ))
        .expect("versions render");
        assert!(versions.contains("example_app ="));
        assert!(versions.contains("registry.example.com/acme/example-app"));

        let import =
            render_body(import_body(Some(&registration), &[])).expect("registration import render");
        assert!(import.contains("resource \"example_app_deployment\" \"this\""));

        let outputs =
            render_body(outputs_body(TerraformTarget::Aws, Some(&registration))).expect("outputs");
        assert!(outputs.contains("example_app_deployment.this.deployment_id"));
    }
}
