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
    import::EmitContext, ErrorData, Network, NetworkSettings, ResourceLifecycle, Result, Stack,
    StackSettings,
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
        if target.is_kubernetes() {
            if resource.lifecycle != ResourceLifecycle::Frozen {
                continue;
            }
            if resource.config.resource_type().as_ref() == "function" {
                continue;
            }
        }

        let emitter = options
            .registry
            .require(&resource.config.resource_type(), platform)?;
        let ctx = EmitContext {
            stack,
            resource,
            resource_id,
            platform,
            stack_settings: &options.stack_settings,
            names: &labels,
        };

        let mut fragment = emitter.emit(&ctx)?;
        if resource.lifecycle == ResourceLifecycle::Live {
            add_live_resource_lifecycle(&mut fragment);
        }
        // Split per-emitter `locals` out of the per-resource file \u2014 they
        // belong in `locals.tf` so reviewers see all locals together.
        let local_contributions = std::mem::take(&mut fragment.locals);
        shared_locals.extend(local_contributions);
        per_resource.insert(resource_id.clone(), fragment);

        let import_ref = emitter.emit_import_ref(&ctx)?;
        imported_resources.push(expr::object([
            ("id", Expression::String(resource_id.to_string())),
            (
                "type",
                Expression::String(resource.config.resource_type().to_string()),
            ),
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

    let network_vars = network_extra_variables(stack, &labels);

    let mut files: IndexMap<String, String> = IndexMap::new();
    files.insert(
        "versions.tf".to_string(),
        render_body(versions_body(target))?,
    );
    files.insert(
        "variables.tf".to_string(),
        render_body(variables_body(target, &network_vars))?,
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

    files.insert("import.tf".to_string(), render_body(import_body())?);
    files.insert("outputs.tf".to_string(), render_body(outputs_body(target))?);
    files.insert("README.md".to_string(), readme_md(stack, target));

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

fn add_live_resource_lifecycle(fragment: &mut TfFragment) {
    for resource in &mut fragment.resource_blocks {
        if let Some(lifecycle) = lifecycle_block_mut(resource) {
            upsert_attr(lifecycle, "ignore_changes", expr::raw("all"));
            upsert_attr(lifecycle, "prevent_destroy", Expression::Bool(true));
        } else {
            resource.body.0.push(nested(block(
                "lifecycle",
                [
                    attr("ignore_changes", expr::raw("all")),
                    attr("prevent_destroy", Expression::Bool(true)),
                ],
            )));
        }
    }
}

fn lifecycle_block_mut(resource: &mut Block) -> Option<&mut Block> {
    resource
        .body
        .0
        .iter_mut()
        .find_map(|structure| match structure {
            Structure::Block(block) if block.identifier.as_str() == "lifecycle" => Some(block),
            _ => None,
        })
}

fn upsert_attr(block: &mut Block, name: &str, value: Expression) {
    for structure in &mut block.body.0 {
        if let Structure::Attribute(attr) = structure {
            if attr.key.as_str() == name {
                attr.expr = value;
                return;
            }
        }
    }
    block.body.0.push(attr(name, value));
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

fn versions_body(target: TerraformTarget) -> Body {
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
    }
    if matches!(target.platform(), alien_core::Platform::Azure) {
        provider_attrs.push(attr(
            "azurerm",
            provider_decl_attr("hashicorp/azurerm", ">= 3.100"),
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

fn variables_body(target: TerraformTarget, network_vars: &NetworkVariables) -> Body {
    let mut blocks: Vec<Structure> = Vec::new();
    blocks.push(nested(variable_block(
        "stack_name",
        "Stable physical-name prefix for resources created by this module.",
        None,
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

    Body::from(blocks)
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
                body: Body::from(vec![Structure::Block(block("features", []))]),
            }));
        }
        _ => {}
    }
    Body::from(structures)
}

fn locals_body(
    target: TerraformTarget,
    stack_settings: &StackSettings,
    imported_resources: Vec<Expression>,
    extra: &IndexMap<String, Expression>,
) -> Result<Body> {
    let mut body: Vec<Structure> = Vec::new();

    body.push(attr(
        "alien_platform",
        Expression::String(target.platform().as_str().to_string()),
    ));
    body.push(attr(
        "alien_target",
        Expression::String(target.name().to_string()),
    ));
    body.push(attr(
        "alien_management_config",
        management_config_expression(target),
    ));
    body.push(attr(
        "alien_stack_settings",
        stack_settings_expression(stack_settings)?,
    ));
    body.push(attr(
        "alien_resources",
        Expression::Array(imported_resources),
    ));

    for (name, value) in extra {
        body.push(attr(name, value.clone()));
    }
    if target.is_kubernetes() {
        body.push(attr(
            "alien_helm_values",
            expr::object([
                (
                    "serviceAccounts",
                    expr::raw("local.alien_helm_service_accounts"),
                ),
                ("stackSettings", expr::raw("local.alien_stack_settings")),
            ]),
        ));
    }

    Ok(Body::from(vec![Structure::Block(Block {
        identifier: Identifier::sanitized("locals"),
        labels: vec![],
        body: Body::from(body),
    })]))
}

fn management_config_expression(target: TerraformTarget) -> Expression {
    let mut object: indexmap::IndexMap<&str, Expression> = indexmap::IndexMap::new();
    object.insert(
        "platform",
        Expression::String(target.platform().as_str().to_string()),
    );
    match target.platform() {
        alien_core::Platform::Aws => {
            object.insert(
                "managingRoleArn",
                expr::raw("data.aws_caller_identity.current.arn"),
            );
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

fn stack_settings_expression(settings: &StackSettings) -> Result<Expression> {
    let value = serde_json::to_value(settings)
        .into_alien_error()
        .map_err(|err| {
            AlienError::new(ErrorData::JsonSerializationFailed {
                reason: format!("failed to serialize StackSettings: {err}"),
            })
        })?;
    Ok(json_to_expression(&value))
}

fn json_to_expression(value: &serde_json::Value) -> Expression {
    match value {
        serde_json::Value::Null => Expression::Null,
        serde_json::Value::Bool(b) => Expression::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Expression::Number(hcl::Number::from(i))
            } else if let Some(f) = n.as_f64() {
                Expression::Number(hcl::Number::from_f64(f).unwrap_or_else(|| hcl::Number::from(0)))
            } else {
                Expression::Null
            }
        }
        serde_json::Value::String(s) => Expression::String(s.clone()),
        serde_json::Value::Array(items) => {
            Expression::Array(items.iter().map(json_to_expression).collect())
        }
        serde_json::Value::Object(map) => {
            let pairs = map.iter().map(|(k, v)| (k.as_str(), json_to_expression(v)));
            expr::object(pairs)
        }
    }
}

fn import_body() -> Body {
    Body::from(vec![Structure::Block(resource_block(
        "terraform_data",
        "alien_stack_import",
        [attr(
            "input",
            expr::object([
                ("platform", expr::raw("local.alien_platform")),
                (
                    "deployment_group_token",
                    expr::raw("var.deployment_group_token"),
                ),
                ("manager_url", expr::raw("var.manager_url")),
                (
                    "management_config",
                    expr::raw("local.alien_management_config"),
                ),
                ("stack_settings", expr::raw("local.alien_stack_settings")),
                ("resources", expr::raw("local.alien_resources")),
            ]),
        )],
    ))])
}

fn outputs_body(target: TerraformTarget) -> Body {
    let mut outputs = vec![
        (
            "alien_target",
            Expression::String(target.name().to_string()),
            "Terraform module target.",
        ),
        (
            "alien_platform",
            expr::raw("local.alien_platform"),
            "Target platform.",
        ),
        (
            "alien_management_config",
            expr::raw("jsonencode(local.alien_management_config)"),
            "Manager import ManagementConfig JSON.",
        ),
        (
            "alien_stack_settings",
            expr::raw("jsonencode(local.alien_stack_settings)"),
            "Manager import StackSettings JSON.",
        ),
        (
            "alien_resources",
            expr::raw("jsonencode(local.alien_resources)"),
            "Manager import resources JSON.",
        ),
    ];
    if target.is_kubernetes() {
        outputs.push((
            "alien_helm_values",
            expr::raw("jsonencode(local.alien_helm_values)"),
            "Helm values JSON for the manager-fetch Kubernetes install.",
        ));
    }

    let blocks: Vec<Structure> = outputs
        .into_iter()
        .map(|(name, value, description)| {
            nested(Block {
                identifier: Identifier::sanitized("output"),
                labels: vec![BlockLabel::String(name.to_string())],
                body: Body::from(vec![
                    attr("value", value),
                    attr("description", Expression::String(description.to_string())),
                ]),
            })
        })
        .collect();

    Body::from(blocks)
}

fn readme_md(stack: &Stack, target: TerraformTarget) -> String {
    format!(
        "# Terraform module - {}\n\n\
Target: `{}`.\n\n\
Run:\n\n\
```bash\nterraform init -backend=false\nterraform validate\nterraform apply -var='stack_name={}' -var='deployment_group_token=...'\n```\n\n\
Outputs `alien_management_config` / `alien_stack_settings` / `alien_resources` for registration via `alien_deployment` (T11) or `alien-deploy register`.\n",
        stack.id(),
        target.name(),
        stack.id()
    )
}
