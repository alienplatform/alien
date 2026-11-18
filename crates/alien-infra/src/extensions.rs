//! A deployment's registries: Alien's built-ins, then the embedding program's registrations, then the
//! extensions its stack lists, in the order the stack lists them.
//!
//! Every registry here is a value built for one stack. Two deployments with different extensions
//! build two sets, so neither sees the other's emitters, importers or controllers.
//!
//! A custom type's emitters and importers run the extension's hooks module. Its controller is the
//! step-protocol proxy, reached through the endpoint the process loaded for the extension.

use crate::core::{
    serialize_controller, PlatformServiceProvider, ResourceController, ResourceRegistry,
};
use crate::import::ImporterRegistry;
use crate::refusal::RefusalContext;
use crate::step_protocol::{ExtensionProxyController, ProxyFactory};
use alien_cloudformation::{CfEmitter, CfExpression, CfRegistry, CfResource};
use alien_core::hooks::{
    EmitInput, EmitMethod, HelmFragmentOutput, HookKind, ImportInput, ImportOutput, LinkOutput,
    TerraformFragment,
};
use alien_core::import::{EmitContext, ImportContext, ResourceImporter};
use alien_core::{
    CustomResourceOutputs, ErrorData, Platform, ResourceOutputs, ResourceStatus, ResourceType,
    Result, Stack, StackResourceState,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_helm::{HelmEmitter, HelmFragment, HelmRegistry, InfrastructureValue};
use alien_hooks_runtime::{HookModule, Limits};
use alien_terraform::{TfEmitter, TfFragment, TfRegistry};
use hcl::{expr::Expression, structure::Block};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

/// Keys of a controller's saved state that only Alien writes: the type tag, the state version,
/// and the executor's stay counter. An importer that sets one is refused.
const RESERVED_STATE_KEYS: [&str; 3] = ["type", "_controllerStateVersion", "internalStayCount"];

/// The registries one deployment uses.
pub struct DeploymentRegistries {
    pub cloudformation: CfRegistry,
    pub terraform: TfRegistry,
    pub helm: HelmRegistry,
    pub importers: ImporterRegistry,
    pub resources: Arc<ResourceRegistry>,
}

/// The registrations of the program that embeds Alien, the layer between Alien's built-ins and
/// a stack's extensions.
///
/// The controller layer is the slot `register_registry_extension` fills, which
/// [`ResourceRegistry::with_built_ins`] applies.
#[derive(Clone, Copy)]
pub struct HostRegistrations {
    pub cloudformation: fn(&mut CfRegistry),
    pub terraform: fn(&mut TfRegistry),
    pub helm: fn(&mut HelmRegistry),
    pub importers: fn(&mut ImporterRegistry),
}

fn none<T>(_: &mut T) {}

impl HostRegistrations {
    /// No host layer: Alien's built-ins and the stack's extensions only.
    pub const NONE: HostRegistrations = HostRegistrations {
        cloudformation: none,
        terraform: none,
        helm: none,
        importers: none,
    };
}

impl Default for HostRegistrations {
    fn default() -> Self {
        Self::NONE
    }
}

/// What a process loaded for a stack's extensions, by extension name: each one's hooks module,
/// and the endpoint of its controller.
#[derive(Clone, Default)]
pub struct LoadedExtensions {
    hooks: HashMap<String, HookModule>,
    controllers: HashMap<String, ProxyFactory>,
    pub(crate) skip_interceptors: bool,
}

impl LoadedExtensions {
    /// The hooks module of `extension`.
    pub fn with_hooks(mut self, extension: impl Into<String>, module: HookModule) -> Self {
        self.hooks.insert(extension.into(), module);
        self
    }

    /// How a deployment's proxies reach the controller of `extension`.
    pub fn with_controller(mut self, extension: impl Into<String>, factory: ProxyFactory) -> Self {
        self.controllers.insert(extension.into(), factory);
        self
    }

    /// The same extensions without their interceptors: the clients of a deployment that runs
    /// with them carry none, while their hooks and controllers still run.
    pub fn without_interceptors(mut self) -> Self {
        self.skip_interceptors = true;
        self
    }

    /// What `provider` loaded for `stack`'s extensions. A stack without extensions never asks,
    /// and a stack with extensions fails unless `provider` loaded the hooks module of each.
    pub fn from_provider(
        stack: &Stack,
        provider: &dyn PlatformServiceProvider,
    ) -> crate::error::Result<Arc<Self>> {
        if stack.extensions.is_empty() {
            return Ok(Arc::default());
        }
        let extensions = provider.extensions().ok_or_else(|| {
            crate::interceptors::not_loaded(
                stack,
                stack
                    .extensions
                    .iter()
                    .map(|manifest| manifest.name.clone())
                    .collect(),
                "the service provider it runs with carries none".to_string(),
            )
        })?;
        extensions.check_loaded(stack)?;
        Ok(extensions)
    }

    /// Fails unless this process loaded the hooks module of every extension `stack` lists. Every
    /// extension's bundle carries one, so a missing module means a loader skipped the extension,
    /// and its interceptors would silently not run.
    pub(crate) fn check_loaded(&self, stack: &Stack) -> crate::error::Result<()> {
        let missing: Vec<String> = stack
            .extensions
            .iter()
            .filter(|manifest| !self.hooks.contains_key(&manifest.name))
            .map(|manifest| manifest.name.clone())
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        Err(crate::interceptors::not_loaded(
            stack,
            missing,
            "their hooks modules are not loaded".to_string(),
        ))
    }

    /// The hooks module of `extension`, if this process loaded one.
    pub(crate) fn hooks(&self, extension: &str) -> Option<&HookModule> {
        self.hooks.get(extension)
    }
}

impl std::fmt::Debug for LoadedExtensions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedExtensions")
            .field("hooks", &self.hooks.keys().collect::<Vec<_>>())
            .field("controllers", &self.controllers)
            .finish()
    }
}

impl DeploymentRegistries {
    /// The registries for `stack`.
    ///
    /// Each extension registers an emitter for every setup path and platform its descriptors
    /// list, an importer for every platform with one, and a controller for every platform it
    /// ships one for. An extension registers only types named after it, so a type Alien, the
    /// host or another extension defines keeps its registration.
    pub fn for_stack(
        stack: &Stack,
        host: &HostRegistrations,
        extensions: &LoadedExtensions,
    ) -> Self {
        let mut cloudformation = CfRegistry::built_in();
        (host.cloudformation)(&mut cloudformation);
        let mut terraform = TfRegistry::built_in();
        (host.terraform)(&mut terraform);
        let mut helm = HelmRegistry::built_in();
        (host.helm)(&mut helm);
        let mut importers = ImporterRegistry::built_in();
        (host.importers)(&mut importers);

        for manifest in &stack.extensions {
            let hooks = Arc::new(ExtensionHooks {
                extension: manifest.name.clone(),
                module: extensions.hooks.get(&manifest.name).cloned(),
            });
            for (resource_type, descriptor) in declared_types(stack, manifest) {
                let hook = |kind, platform| ExtensionHook {
                    hooks: hooks.clone(),
                    kind,
                    resource_type: resource_type.clone(),
                    platform,
                };
                for &platform in &descriptor.setup.cloudformation {
                    cloudformation.register(
                        resource_type.clone(),
                        platform,
                        hook(HookKind::CloudformationEmitter, platform),
                    );
                }
                for &platform in &descriptor.setup.terraform {
                    terraform.register(
                        resource_type.clone(),
                        platform,
                        hook(HookKind::TerraformEmitter, platform),
                    );
                }
                for &platform in &descriptor.setup.helm {
                    helm.register(
                        resource_type.clone(),
                        platform,
                        hook(HookKind::HelmEmitter, platform),
                    );
                }
                for &platform in &descriptor.setup.importers {
                    importers.register(
                        resource_type.clone(),
                        platform,
                        hook(HookKind::Importer, platform),
                    );
                }
            }
        }

        Self {
            cloudformation,
            terraform,
            helm,
            importers,
            resources: Arc::new(ResourceRegistry::with_built_ins_and_extensions(
                stack, extensions,
            )),
        }
    }
}

impl ResourceRegistry {
    /// Alien's built-in controllers, the host's, and the step-protocol proxy for each custom
    /// type and platform an extension in `stack` ships a controller for and `extensions` has an
    /// endpoint for.
    pub fn with_built_ins_and_extensions(stack: &Stack, extensions: &LoadedExtensions) -> Self {
        let mut registry = Self::with_built_ins();
        registry.add_extensions(stack, extensions, |_, _| true);
        registry
    }

    /// The registry of a deployment moving from the `current` release to the `target` one, each
    /// a stack with what this process loaded for its extensions. A type and platform the target
    /// release ships a controller for gets the target release's; one only the current release
    /// ships, which the update removes, keeps the current release's, so its resources can still
    /// be deleted. That includes a type the target release keeps for other platforms only.
    pub fn with_built_ins_and_releases(
        target: (&Stack, &LoadedExtensions),
        current: (&Stack, &LoadedExtensions),
    ) -> Self {
        let mut registry = Self::with_built_ins();
        registry.add_extensions(target.0, target.1, |_, _| true);
        registry.add_extensions(current.0, current.1, |resource_type, platform| {
            target
                .0
                .descriptor(resource_type)
                .map_or(true, |descriptor| {
                    !descriptor.controller_platforms.contains(&platform)
                })
        });
        registry
    }

    /// Registers the controllers of `stack`'s extensions for the types and platforms `include`
    /// accepts.
    fn add_extensions(
        &mut self,
        stack: &Stack,
        extensions: &LoadedExtensions,
        include: impl Fn(&ResourceType, Platform) -> bool,
    ) {
        for manifest in &stack.extensions {
            let factory = extensions.controllers.get(&manifest.name);
            for (resource_type, descriptor) in declared_types(stack, manifest) {
                for &platform in &descriptor.controller_platforms {
                    if !include(resource_type, platform) {
                        continue;
                    }
                    match factory {
                        Some(factory) => self.register_controller_factory(
                            resource_type.clone(),
                            platform,
                            Box::new(factory.clone()),
                        ),
                        None => self.register_unloaded_extension(
                            resource_type.clone(),
                            platform,
                            manifest.name.clone(),
                        ),
                    }
                }
            }
        }
    }
}

/// Runs `work` on tokio's blocking pool. Async code calls anything that may run an extension's
/// hooks (an importer, an emitter, a modifier) through this: a hook call blocks its thread for up
/// to its deadline, and a worker thread blocked that long stalls every task scheduled on it.
///
/// A panic in `work` comes back as [`crate::error::ErrorData::HooksStopped`] instead of
/// unwinding into the caller's task, so a server answers that one request with an error and
/// keeps serving.
pub async fn run_hooks<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> crate::error::Result<T> {
    tokio::task::spawn_blocking(work).await.map_err(|error| {
        let reason = if error.is_panic() {
            let panic = error.into_panic();
            let message = panic
                .downcast_ref::<&str>()
                .map(|message| message.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic without a message".to_string());
            format!("panicked: {message}")
        } else {
            "the runtime shut down before it ran".to_string()
        };
        AlienError::new(crate::error::ErrorData::HooksStopped { reason })
    })
}

/// The environment variables a workload that links `resource_id`, a resource of an extension's
/// type, gets: the type's link run on the resource's `outputs`, without their type tag. A type
/// whose descriptor declares no variables gets none, and its link doesn't run.
///
/// The link runs in the extension's module, so what it returns is checked: a name the descriptor
/// doesn't declare is refused, since a release checked only the declared names.
pub(crate) async fn link_environment(
    stack: &Stack,
    extensions: Option<&LoadedExtensions>,
    resource_type: &ResourceType,
    resource_id: &str,
    outputs: Value,
) -> Result<Vec<(String, String)>> {
    let Some((manifest, descriptor)) = stack.extensions.iter().find_map(|manifest| {
        declared_types(stack, manifest)
            .find(|(declared, _)| *declared == resource_type)
            .map(|(_, descriptor)| (manifest, descriptor))
    }) else {
        return Ok(Vec::new());
    };
    if descriptor.link.env.is_empty() {
        return Ok(Vec::new());
    }
    let what = format!(
        "the link of extension '{}' for {resource_type} '{resource_id}'",
        manifest.name
    );
    let module = ExtensionHooks {
        extension: manifest.name.clone(),
        module: extensions.and_then(|loaded| loaded.hooks(&manifest.name).cloned()),
    }
    .module(&what)?
    .clone();
    let entry = module
        .manifest
        .hooks
        .iter()
        .find(|entry| {
            entry.kind == HookKind::Link
                && entry.routing.get("resourceType").and_then(Value::as_str)
                    == Some(resource_type.as_ref())
        })
        .ok_or_else(|| {
            crate::refusal::refusal(ErrorData::GenericError {
                message: format!(
                    "{what} can't run: the extension's descriptor declares its variables, and its hooks module has no link"
                ),
            })
        })?
        .name
        .clone();
    let input = serde_json::to_vec(&outputs).into_alien_error().context(
        ErrorData::JsonSerializationFailed {
            reason: format!("the input of {what} did not serialize"),
        },
    )?;
    let output = run_hooks(move || module.call(&entry, &input, &Limits::for_kind(HookKind::Link)))
        .await
        .context(ErrorData::GenericError {
            message: format!("{what} did not run"),
        })?
        .context_as_wrapped(ErrorData::GenericError {
            message: format!("{what} failed"),
        })?;
    let link: LinkOutput = serde_json::from_slice(&output).into_alien_error().context(
        ErrorData::JsonDeserializationFailed {
            reason: format!("the output of {what} did not read"),
        },
    )?;
    if let Some(name) = link
        .env
        .keys()
        .find(|name| !descriptor.link.env.contains(name))
    {
        return Err(crate::refusal::refusal(ErrorData::GenericError {
            message: format!(
                "{what} set `{name}`, which the type's descriptor doesn't declare. Rebuild the extension with `alien extensions build`"
            ),
        }));
    }
    Ok(link.env.into_iter().collect())
}

/// The types `manifest` defines that the stack resolves to it: custom, and named after it.
/// Registries keep the last registration, so any other type would replace one.
fn declared_types<'a>(
    stack: &'a Stack,
    manifest: &'a alien_core::ExtensionManifest,
) -> impl Iterator<Item = (&'a ResourceType, &'a alien_core::ResourceTypeDescriptor)> {
    manifest
        .resource_types
        .iter()
        .filter(move |(resource_type, descriptor)| {
            stack
                .descriptor(resource_type)
                .is_some_and(|found| std::ptr::eq(found, *descriptor))
        })
}

/// One extension's hooks module, as its emitters, importers and modifiers call it.
pub(crate) struct ExtensionHooks {
    pub(crate) extension: String,
    /// `None` when this process has not loaded the module; a call then fails with that reason.
    module: Option<HookModule>,
}

impl ExtensionHooks {
    /// The extension's module, for running `what`. Fails when this process has not loaded it, or
    /// loaded another extension's module under its name.
    pub(crate) fn module(&self, what: &str) -> Result<&HookModule> {
        let module = self.module.as_ref().ok_or_else(|| {
            crate::refusal::refusal(ErrorData::GenericError {
                message: format!(
                    "{what} can't run: this process has not loaded the extension's hooks module"
                ),
            })
        })?;
        if module.manifest.extension != self.extension {
            return Err(crate::refusal::refusal(ErrorData::GenericError {
                message: format!(
                    "{what} can't run: the module loaded for it is extension '{}'",
                    module.manifest.extension
                ),
            }));
        }
        Ok(module)
    }
}

/// The hook of one kind for one custom type and platform: an emitter for its setup path, or
/// an importer.
struct ExtensionHook {
    hooks: Arc<ExtensionHooks>,
    kind: HookKind,
    resource_type: ResourceType,
    platform: Platform,
}

impl ExtensionHook {
    /// Runs the module's hook for this kind, type and platform on `input`, and reads its output.
    fn call<T: DeserializeOwned>(&self, input: &impl Serialize) -> Result<T> {
        let what = format!(
            "the {} of extension '{}' for {} on {}",
            kind_name(self.kind),
            self.hooks.extension,
            self.resource_type,
            self.platform
        );
        let module = self.hooks.module(&what)?;
        let platform = serde_json::to_value(self.platform)
            .into_alien_error()
            .context(ErrorData::JsonSerializationFailed {
                reason: "a platform did not serialize".to_string(),
            })?;
        let entry = module
            .manifest
            .hooks
            .iter()
            .find(|entry| {
                entry.kind == self.kind
                    && entry.routing.get("resourceType").and_then(Value::as_str)
                        == Some(self.resource_type.as_ref())
                    && entry.routing.get("platform") == Some(&platform)
            })
            .ok_or_else(|| {
                crate::refusal::refusal(ErrorData::GenericError {
                    message: format!(
                        "{what} can't run: the extension's descriptor lists it, and its hooks module has no such hook"
                    ),
                })
            })?;
        let input = serde_json::to_vec(input).into_alien_error().context(
            ErrorData::JsonSerializationFailed {
                reason: format!("the input of {what} did not serialize"),
            },
        )?;
        let output = module
            .call(&entry.name, &input, &Limits::for_kind(self.kind))
            .context_as_wrapped(ErrorData::GenericError {
                message: format!("{what} failed"),
            })?;
        serde_json::from_slice(&output).into_alien_error().context(
            ErrorData::JsonDeserializationFailed {
                reason: format!("the output of {what} did not read"),
            },
        )
    }

    fn emit<T: DeserializeOwned>(&self, method: EmitMethod, ctx: &EmitContext<'_>) -> Result<T> {
        self.call(&EmitInput {
            method,
            resource_id: ctx.resource_id.to_string(),
            platform: ctx.platform,
            targets_kubernetes: ctx.targets_kubernetes,
            // The other resources' external bindings can carry credentials, which an emitter
            // could copy into what it writes, so it gets the settings the controller would.
            stack_settings: crate::step_protocol::stack_settings_for_extension(ctx.stack_settings),
            names: ctx.names.clone(),
            resource: entry_json(ctx.resource)?,
        })
    }
}

fn kind_name(kind: HookKind) -> &'static str {
    match kind {
        HookKind::CloudformationEmitter => "CloudFormation emitter",
        HookKind::TerraformEmitter => "Terraform emitter",
        HookKind::HelmEmitter => "Helm emitter",
        HookKind::Importer => "importer",
        _ => "hook",
    }
}

/// A stack entry in `ResourceEntry`'s serde form, with its config's `type` tag, as the
/// extension SDK reads it.
fn entry_json(entry: &alien_core::ResourceEntry) -> Result<Value> {
    serde_json::to_value(entry)
        .into_alien_error()
        .context(ErrorData::JsonSerializationFailed {
            reason: "a resource entry did not serialize".to_string(),
        })
}

impl CfEmitter for ExtensionHook {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let resources: Vec<(String, CfResource)> = self.emit(EmitMethod::Emit, ctx)?;
        Ok(resources
            .into_iter()
            .map(|(logical_id, mut resource)| {
                resource.logical_id = logical_id;
                resource
            })
            .collect())
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        self.emit(EmitMethod::EmitImportRef, ctx)
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        self.emit(EmitMethod::EmitBindingRef, ctx)
    }
}

impl TfEmitter for ExtensionHook {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let fragment: TerraformFragment = ExtensionHook::emit(self, EmitMethod::Emit, ctx)?;
        let mut emitted = TfFragment::default();
        emitted.minimum_setup_import_format_version = fragment.minimum_setup_import_format_version;
        emitted.variable_blocks = hcl_blocks(&fragment.variable_blocks)?;
        emitted.moved_blocks = hcl_blocks(&fragment.moved_blocks)?;
        emitted.resource_blocks = hcl_blocks(&fragment.resource_blocks)?;
        emitted.data_blocks = hcl_blocks(&fragment.data_blocks)?;
        emitted.ephemeral_blocks = hcl_blocks(&fragment.ephemeral_blocks)?;
        for (name, expression) in &fragment.locals {
            emitted
                .locals
                .insert(name.clone(), hcl_expression(expression)?);
        }
        Ok(emitted)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let expression: String = self.emit(EmitMethod::EmitImportRef, ctx)?;
        hcl_expression(&expression)
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let expression: Option<String> = self.emit(EmitMethod::EmitBindingRef, ctx)?;
        expression.as_deref().map(hcl_expression).transpose()
    }
}

fn hcl_blocks(text: &str) -> Result<Vec<Block>> {
    let body = hcl::parse(text)
        .into_alien_error()
        .context_refused(ErrorData::GenericError {
            message: "an extension's Terraform emitter returned HCL that does not parse"
                .to_string(),
        })?;
    if body.attributes().next().is_some() {
        return Err(crate::refusal::refusal(ErrorData::GenericError {
            message: "an extension's Terraform emitter returned attributes where blocks belong"
                .to_string(),
        }));
    }
    Ok(body.into_blocks().collect())
}

fn hcl_expression(text: &str) -> Result<Expression> {
    Expression::from_str(text)
        .into_alien_error()
        .context_refused(ErrorData::GenericError {
            message: format!(
            "an extension's Terraform emitter returned an expression that does not parse: {text}"
        ),
        })
}

impl HelmEmitter for ExtensionHook {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<HelmFragment> {
        let fragment: HelmFragmentOutput = ExtensionHook::emit(self, EmitMethod::Emit, ctx)?;
        Ok(HelmFragment {
            infrastructure: fragment.infrastructure.map(|value| InfrastructureValue {
                id: value.id,
                binding_type: value.binding_type,
                service: value.service,
                fields: value.fields,
            }),
            extra_templates: fragment.extra_templates,
        })
    }
}

impl ResourceImporter for ExtensionHook {
    type ImportData = Value;

    /// Runs the extension's importer in its module, and stores what it returns for the
    /// controller to load on its first step, so storing an import never waits on the controller.
    fn import(&self, data: Value, ctx: &ImportContext<'_>) -> Result<StackResourceState> {
        let imported: ImportOutput = self.call(&ImportInput {
            resource_id: ctx.resource_id.to_string(),
            platform: ctx.platform,
            region: ctx.region.to_string(),
            // What the importer returns reaches the controller, which may run outside the
            // customer's cloud, so it gets the settings the controller would.
            stack_settings: crate::step_protocol::stack_settings_for_extension(ctx.stack_settings),
            management_config: ctx.management_config.cloned(),
            resource: entry_json(ctx.resource)?,
            import_data: data,
        })?;
        imported_state(&self.resource_type, imported, ctx)
    }

    fn merge_reimport(
        &self,
        existing: StackResourceState,
        imported: StackResourceState,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        reimported(existing, imported, ctx)
    }
}

/// A setup rerun's import of a resource the deployment already holds. The controller owns the
/// resource once setup handed it over: its view, the status, the transitions it has not seen yet
/// and its outputs stay, and one that never loaded the first import loads the new one. The
/// controller a manual retry resumes from gets the same merge.
fn reimported(
    existing: StackResourceState,
    imported: StackResourceState,
    ctx: &ImportContext<'_>,
) -> Result<StackResourceState> {
    let Some(saved) = existing.internal_state.as_ref() else {
        return Ok(imported);
    };
    let proxy = |saved: &Value, which: &str| {
        crate::core::deserialize_controller(saved.clone())
            .ok()
            .and_then(|controller| {
                controller
                    .as_any()
                    .downcast_ref::<ExtensionProxyController>()
                    .cloned()
            })
            .ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!(
                        "the {which} state of '{}' is not an extension resource's",
                        ctx.resource_id
                    ),
                })
            })
    };
    let fresh = imported
        .internal_state
        .as_ref()
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: format!(
                    "the imported state of '{}' has no controller state to merge",
                    ctx.resource_id
                ),
            })
        })
        .and_then(|saved| proxy(saved, "imported"))?;
    let save = |merged: &ExtensionProxyController| {
        serialize_controller(merged).into_alien_error().context(
            ErrorData::JsonSerializationFailed {
                reason: "the merged controller state did not serialize".to_string(),
            },
        )
    };
    let merged = proxy(saved, "existing")?.reimported(fresh.clone());
    let last_failed_state = existing
        .last_failed_state
        .as_ref()
        .map(|saved| save(&proxy(saved, "last failed")?.reimported(fresh)))
        .transpose()?;
    Ok(StackResourceState {
        internal_state: Some(save(&merged)?),
        outputs: merged.get_outputs(),
        config: imported.config,
        last_failed_state,
        ..existing
    })
}

/// The resource state an importer's output describes: its fields wait on the proxy until the
/// controller loads them, and its outputs carry the type tag.
fn imported_state(
    resource_type: &ResourceType,
    imported: ImportOutput,
    ctx: &ImportContext<'_>,
) -> Result<StackResourceState> {
    if imported.status != ResourceStatus::Running {
        return Err(crate::refusal::refusal(ErrorData::GenericError {
            message: format!(
                "the importer of {resource_type} returned the status {:?}; setup hands a resource over running",
                imported.status
            ),
        }));
    }
    let Value::Object(fields) = &imported.internal_state else {
        return Err(crate::refusal::refusal(ErrorData::GenericError {
            message: format!(
                "the importer of {resource_type} returned controller fields that are not a JSON object"
            ),
        }));
    };
    if let Some(key) = RESERVED_STATE_KEYS
        .iter()
        .find(|key| fields.contains_key(**key))
    {
        return Err(crate::refusal::refusal(ErrorData::GenericError {
            message: format!(
                "the importer of {resource_type} set '{key}' in the controller's fields, which only Alien writes"
            ),
        }));
    }
    let outputs = imported
        .outputs
        .map(|outputs| {
            CustomResourceOutputs::from_untagged(resource_type.as_ref(), outputs)
                .map_err(|reason| AlienError::new(ErrorData::JsonDeserializationFailed { reason }))
        })
        .transpose()?
        .map(ResourceOutputs::new);
    let proxy = ExtensionProxyController::imported(
        imported.internal_state,
        imported.status,
        outputs.clone(),
    );
    let internal_state = serialize_controller(&proxy).into_alien_error().context(
        ErrorData::JsonSerializationFailed {
            reason: "the imported controller state did not serialize".to_string(),
        },
    )?;
    Ok(StackResourceState::builder()
        .resource_type(resource_type.to_string())
        .status(imported.status)
        .config(ctx.resource.config.clone())
        .internal_state(internal_state)
        .maybe_outputs(outputs)
        .lifecycle(ctx.resource.lifecycle)
        .dependencies(ctx.resource.combined_dependencies())
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::step_protocol::protocol::StepRequest;
    use alien_core::{ResourceEntry, ResourceLifecycle, ResourceStatus, StackSettings};
    use serde_json::json;

    fn entry() -> ResourceEntry {
        serde_json::from_value(json!({
            "config": { "type": "acme/dns-zone", "id": "zone", "domain": "a.example" },
            "lifecycle": "frozen",
            "dependencies": []
        }))
        .unwrap()
    }

    fn import(internal_state: Value) -> Result<StackResourceState> {
        import_as(internal_state, ResourceStatus::Running, "Z1")
    }

    /// Runs `with` in the import context of the zone.
    fn in_context<T>(with: impl FnOnce(&ImportContext<'_>) -> T) -> T {
        let entry = entry();
        let settings = StackSettings::default();
        with(&ImportContext {
            resource_id: "zone",
            platform: Platform::Aws,
            region: "us-east-1",
            stack_settings: &settings,
            management_config: None,
            resource: &entry,
        })
    }

    fn import_as(
        internal_state: Value,
        status: ResourceStatus,
        zone_id: &str,
    ) -> Result<StackResourceState> {
        in_context(|ctx| {
            imported_state(
                &ResourceType::from("acme/dns-zone"),
                ImportOutput {
                    internal_state,
                    status,
                    outputs: Some(json!({ "zoneId": zone_id, "domain": "a.example" })),
                },
                ctx,
            )
        })
    }

    /// A stack whose extension `name` declares `resource_type`, with a CloudFormation emitter and
    /// an importer on AWS.
    fn manifest(name: &str, resource_type: &str) -> alien_core::ExtensionManifest {
        serde_json::from_value(json!({
            "name": name,
            "version": "1.0.0",
            "digest": "sha256:0",
            "resourceTypes": {
                resource_type: {
                    "lifecycles": ["frozen"],
                    "holdsData": false,
                    "setup": { "cloudformation": ["aws"], "terraform": [], "helm": [], "importers": ["aws"] },
                    "controllerPlatforms": [],
                    "permissionSets": [],
                    "link": { "permissionSets": [] },
                    "schemas": { "config": {}, "outputs": {} }
                }
            }
        }))
        .unwrap()
    }

    /// Emits `resource_id` through the registry's CloudFormation emitter for its type on AWS.
    fn emit(stack: &Stack, registries: &DeploymentRegistries, resource_id: &str) -> Result<()> {
        let resource = &stack.resources[resource_id];
        let settings = StackSettings::default();
        let names = [(resource_id.to_string(), "Data".to_string())]
            .into_iter()
            .collect();
        registries
            .cloudformation
            .require(&resource.config.resource_type(), Platform::Aws)?
            .emit_resources(&EmitContext {
                stack,
                resource,
                resource_id,
                platform: Platform::Aws,
                targets_kubernetes: false,
                stack_settings: &settings,
                names: &names,
                pins: &alien_core::SetupPins::default(),
            })
            .map(|_| ())
    }

    #[test]
    fn an_extension_keeps_away_from_types_it_does_not_own() {
        let mut stack = alien_core::Stack::new("app".to_string())
            .add(
                alien_core::Storage::new("data".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        stack.resources.insert("zone".to_string(), entry());
        stack.extensions = vec![
            manifest("evil", "acme/dns-zone"),
            manifest("acme", "acme/dns-zone"),
            manifest("other", "storage"),
        ];
        let registries = DeploymentRegistries::for_stack(
            &stack,
            &HostRegistrations::NONE,
            &LoadedExtensions::default(),
        );
        // Alien's storage emitter still renders the bucket; the module that claimed the type
        // was never loaded, so reaching its hook would fail.
        emit(&stack, &registries, "data").unwrap();
        // The zone's hook is the extension named by the type's prefix, not the first to list it.
        let error = emit(&stack, &registries, "zone").unwrap_err();
        assert!(
            error.message.contains("extension 'acme'"),
            "{}",
            error.message
        );
    }

    #[derive(Debug)]
    struct Unused;

    #[async_trait::async_trait]
    impl crate::step_protocol::Transport for Unused {
        async fn step(
            &self,
            _request: &crate::step_protocol::protocol::StepRequest,
            _timeout: std::time::Duration,
        ) -> std::result::Result<
            crate::step_protocol::protocol::StepResponse,
            crate::step_protocol::TransportFailure,
        > {
            unreachable!("the test never steps")
        }
    }

    fn with_controller(name: &str) -> LoadedExtensions {
        LoadedExtensions::default()
            .with_controller(name, ProxyFactory::new("dep", Arc::new(Unused), None))
    }

    /// A stack whose `acme` extension ships a controller on AWS for each of `types`.
    fn release(types: &[&str]) -> Stack {
        release_on(types, &[Platform::Aws])
    }

    /// A stack whose `acme` extension ships a controller on `platforms` for each of `types`.
    fn release_on(types: &[&str], platforms: &[Platform]) -> Stack {
        let mut manifest = manifest("acme", types[0]);
        for resource_type in types {
            let mut descriptor = manifest.resource_types[0].clone();
            descriptor.controller_platforms = platforms.to_vec();
            manifest
                .resource_types
                .insert(ResourceType::from(*resource_type), descriptor);
        }
        let mut stack = alien_core::Stack::new("app".to_string()).build();
        stack.extensions = vec![manifest];
        stack
    }

    /// The target release's endpoint serves the types it defines; the current release's serves
    /// only the type the target release dropped.
    #[test]
    fn a_type_the_target_release_dropped_keeps_the_current_releases_controller() {
        let target = release(&["acme/dns-zone"]);
        let current = release(&["acme/dns-zone", "acme/record"]);
        let zone = ResourceType::from("acme/dns-zone");
        let record = ResourceType::from("acme/record");

        let registry = ResourceRegistry::with_built_ins_and_releases(
            (&target, &LoadedExtensions::default()),
            (&current, &with_controller("acme")),
        );
        let error = registry
            .get_controller(zone.clone(), Platform::Aws)
            .unwrap_err();
        assert_eq!(error.code, "EXTENSION_CONTROLLER_NOT_LOADED");
        registry
            .get_controller(record.clone(), Platform::Aws)
            .unwrap();

        let registry = ResourceRegistry::with_built_ins_and_releases(
            (&target, &with_controller("acme")),
            (&current, &LoadedExtensions::default()),
        );
        registry.get_controller(zone, Platform::Aws).unwrap();
        let error = registry.get_controller(record, Platform::Aws).unwrap_err();
        assert_eq!(error.code, "EXTENSION_CONTROLLER_NOT_LOADED");
    }

    /// A target release that keeps a type but stops shipping its controller on a platform leaves
    /// that platform the current release's controller, so a resource there can still be deleted.
    #[test]
    fn a_platform_the_target_release_dropped_keeps_the_current_releases_controller() {
        let target = release_on(&["acme/dns-zone"], &[Platform::Gcp]);
        let current = release_on(&["acme/dns-zone"], &[Platform::Aws, Platform::Gcp]);
        let zone = ResourceType::from("acme/dns-zone");

        let registry = ResourceRegistry::with_built_ins_and_releases(
            (&target, &LoadedExtensions::default()),
            (&current, &with_controller("acme")),
        );
        registry
            .get_controller(zone.clone(), Platform::Aws)
            .unwrap();
        let error = registry.get_controller(zone, Platform::Gcp).unwrap_err();
        assert_eq!(error.code, "EXTENSION_CONTROLLER_NOT_LOADED");
    }

    /// On a single-threaded runtime, a hook running in place would keep every other task waiting.
    #[tokio::test]
    async fn hooks_run_while_other_tasks_make_progress() {
        let (sent, received) = std::sync::mpsc::channel();
        tokio::spawn(async move { sent.send(()).unwrap() });
        let other_task_ran = run_hooks(move || {
            received
                .recv_timeout(std::time::Duration::from_secs(5))
                .is_ok()
        })
        .await
        .unwrap();
        assert!(other_task_ran);
    }

    /// A panic in the hooks' work is an error for the caller to answer, not a panic of its task.
    #[tokio::test]
    async fn a_panic_in_the_hooks_work_is_an_error() {
        let error = run_hooks(|| -> () { panic!("the emitter fell over") })
            .await
            .unwrap_err();
        assert_eq!(error.code, "HOOKS_STOPPED");
        assert!(!error.retryable);
        assert!(error.internal);
        assert!(
            error.message.contains("panicked: the emitter fell over"),
            "{}",
            error.message
        );
        let error = run_hooks(|| -> () { panic!("{} fell over", "the importer") })
            .await
            .unwrap_err();
        assert!(error.message.contains("panicked: the importer fell over"));
    }

    #[test]
    fn an_importer_that_writes_a_reserved_key_is_refused() {
        for key in RESERVED_STATE_KEYS {
            let error = import(json!({ "state": "ready", key: 1 })).unwrap_err();
            assert!(error.message.contains(key), "{}", error.message);
        }
        let error = import(json!(["ready"])).unwrap_err();
        assert!(
            error.message.contains("not a JSON object"),
            "{}",
            error.message
        );
    }

    /// Setup creates a resource whole, so an importer describes it running; any other status
    /// would start the controller from a state setup never left it in.
    #[test]
    fn an_importer_s_status_other_than_running_is_refused() {
        for status in [
            ResourceStatus::Provisioning,
            ResourceStatus::ProvisionFailed,
            ResourceStatus::Deleting,
            ResourceStatus::Deleted,
        ] {
            let error = import_as(json!({ "state": "ready" }), status, "Z1").unwrap_err();
            assert!(
                error.message.contains("running"),
                "{status:?}: {}",
                error.message
            );
        }
    }

    /// A setup rerun imports a resource the controller already runs: the controller's view, the
    /// status and the transitions it has not seen stay, and the import brings only its outputs.
    #[test]
    fn a_setup_rerun_keeps_the_controller_s_runtime_state() {
        let view = json!({
            "state": { "state": "ready", "zoneId": "Z1" }, "status": "running",
            "progress": null, "outputs": null, "bindingParams": null,
            "onFailureStatus": "refresh-failed",
            "updateStart": { "outcome": "allowed", "status": "updating" },
            "deleteStart": { "outcome": "allowed", "status": "deleting" },
            "teardownStart": { "outcome": "allowed", "status": "deleting" }
        });
        let running: ExtensionProxyController = serde_json::from_value(json!({
            "status": "updating", "remote": view, "pending": ["update-start"],
            "inFlightStepId": "step-1", "pendingImport": null
        }))
        .unwrap();
        let mut existing = import(json!({ "state": "ready", "zoneId": "Z1" })).unwrap();
        existing.status = ResourceStatus::Updating;
        existing.internal_state = Some(serialize_controller(&running).unwrap());

        let again = import_as(
            json!({ "state": "ready", "zoneId": "Z2" }),
            ResourceStatus::Running,
            "Z2",
        )
        .unwrap();
        let merged = in_context(|ctx| reimported(existing.clone(), again.clone(), ctx)).unwrap();
        assert_eq!(merged.status, ResourceStatus::Updating);
        let saved = merged.internal_state.as_ref().unwrap();
        assert_eq!(saved["status"], "updating");
        assert_eq!(saved["remote"], view);
        assert_eq!(saved["pending"], json!(["update-start"]));
        assert_eq!(saved["inFlightStepId"], "step-1");
        assert!(saved["pendingImport"].is_null());
        // The outputs are the controller's, which holds the first import, so the state does
        // not report fields the controller never loaded.
        assert!(merged.outputs.is_none(), "{:?}", merged.outputs);
        // The config is the one setup imported the resource with.
        let mut older = existing.clone();
        older.config = serde_json::from_value(
            json!({ "type": "acme/dns-zone", "id": "zone", "domain": "old.example" }),
        )
        .unwrap();
        let merged = in_context(|ctx| reimported(older, again.clone(), ctx)).unwrap();
        assert_eq!(merged.config, again.config);

        // A controller that never loaded the first import loads the new one.
        let unloaded = import(json!({ "state": "ready", "zoneId": "Z1" })).unwrap();
        let merged = in_context(|ctx| reimported(unloaded, again, ctx)).unwrap();
        assert_eq!(
            merged.internal_state.unwrap()["pendingImport"],
            json!({ "state": "ready", "zoneId": "Z2" })
        );
    }

    /// A deployment whose record of the resource was dropped, by a forced delete for one,
    /// takes the import as it is.
    #[test]
    fn a_setup_rerun_over_a_resource_without_controller_state_takes_the_import() {
        let mut existing = import(json!({ "state": "ready", "zoneId": "Z1" })).unwrap();
        existing.internal_state = None;
        let again = import_as(
            json!({ "state": "ready", "zoneId": "Z2" }),
            ResourceStatus::Running,
            "Z2",
        )
        .unwrap();
        let merged = in_context(|ctx| reimported(existing, again.clone(), ctx)).unwrap();
        assert_eq!(
            serde_json::to_value(&merged).unwrap(),
            serde_json::to_value(&again).unwrap()
        );
    }

    /// Answers the `n`th step (from 0) with what the closure builds, and keeps the import each
    /// request carried.
    struct Scripted {
        reply: Box<dyn Fn(usize, &StepRequest) -> Value + Send + Sync>,
        imports: std::sync::Mutex<Vec<Option<Value>>>,
    }

    impl std::fmt::Debug for Scripted {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Scripted")
        }
    }

    #[async_trait::async_trait]
    impl crate::step_protocol::Transport for Scripted {
        async fn step(
            &self,
            request: &StepRequest,
            _timeout: std::time::Duration,
        ) -> std::result::Result<
            crate::step_protocol::protocol::StepResponse,
            crate::step_protocol::TransportFailure,
        > {
            let mut imports = self.imports.lock().unwrap();
            let reply = (self.reply)(imports.len(), request);
            imports.push(request.pending_import.clone());
            Ok(serde_json::from_value(reply).unwrap())
        }
    }

    /// The first step fails before the controller loads anything, with `retryable`; every
    /// later one loads the import it carries.
    fn fails_first(retryable: bool) -> Arc<Scripted> {
        Arc::new(Scripted {
            reply: Box::new(move |n, request| {
                let mut controller = json!({
                    "state": { "state": "ready" }, "status": "running", "progress": null,
                    "outputs": null, "bindingParams": null, "onFailureStatus": "refresh-failed",
                    "updateStart": { "outcome": "allowed", "status": "updating" },
                    "deleteStart": { "outcome": "allowed", "status": "deleting" },
                    "teardownStart": { "outcome": "allowed", "status": "deleting" }
                });
                if n == 0 {
                    controller["state"] = Value::Null;
                    return json!({ "protocol": 1, "stepId": request.step_id, "result": "failed",
                        "controller": controller,
                        "error": { "code": "UNREACHABLE_API", "message": "m", "retryable": retryable } });
                }
                json!({ "protocol": 1, "stepId": request.step_id, "result": "stepped",
                    "controller": controller, "suggestedDelayMs": 1000, "heartbeats": [] })
            }),
            imports: Default::default(),
        })
    }

    /// Steps the zone once through `transport`.
    async fn step_zone(transport: Arc<Scripted>, zone: StackResourceState) -> StackResourceState {
        let mut stack = Stack::new("app".to_string()).build();
        stack.resources.insert("zone".to_string(), entry());
        let mut registry = ResourceRegistry::with_built_ins();
        registry.register_controller_factory(
            ResourceType::from("acme/dns-zone"),
            Platform::Test,
            Box::new(ProxyFactory::new("dep", transport, None)),
        );
        let config = alien_core::DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(Default::default())
            .allow_frozen_changes(false)
            .build();
        let executor = crate::StackExecutor::builder(&stack, alien_core::ClientConfig::Test)
            .deployment_config(&config)
            .resource_registry(Arc::new(registry))
            .service_provider(Arc::new(crate::MockPlatformServiceProvider::new()))
            .build()
            .unwrap();
        let mut state = alien_core::StackState::new(Platform::Test);
        state.resources.insert("zone".to_string(), zone);
        executor.step(state).await.unwrap().next_state.resources["zone"].clone()
    }

    fn imported_zone(zone_id: &str) -> StackResourceState {
        let mut zone = import_as(
            json!({ "state": "ready", "zoneId": zone_id }),
            ResourceStatus::Running,
            zone_id,
        )
        .unwrap();
        zone.controller_platform = Some(Platform::Test);
        zone
    }

    fn zone_id(outputs: Option<&ResourceOutputs>) -> Value {
        serde_json::to_value(outputs.expect("outputs")).unwrap()["zoneId"].clone()
    }

    /// A step that failed before the controller loaded the first import leaves it unloaded: a
    /// setup rerun's import is the one the next step carries, and the one the outputs report.
    #[tokio::test]
    async fn a_setup_rerun_after_a_failed_first_step_sends_the_new_import() {
        let transport = fails_first(true);
        let failed = step_zone(transport.clone(), imported_zone("Z1")).await;
        assert!(failed.error.is_some());

        let merged = in_context(|ctx| reimported(failed, imported_zone("Z2"), ctx)).unwrap();
        assert_eq!(zone_id(merged.outputs.as_ref()), "Z2");
        assert!(merged.internal_state.as_ref().unwrap()["inFlightStepId"].is_null());

        step_zone(transport.clone(), merged).await;
        let imports = transport.imports.lock().unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[1].as_ref().unwrap()["zoneId"], "Z2");
    }

    /// A controller with no state has loaded nothing, even with no import pending: a setup rerun's
    /// import is the one its next step carries.
    #[tokio::test]
    async fn a_setup_rerun_reaches_a_controller_that_never_loaded_without_a_pending_import() {
        let transport = fails_first(true);
        let mut failed = step_zone(transport.clone(), imported_zone("Z1")).await;
        let saved = failed.internal_state.as_ref().unwrap();
        assert!(saved["remote"].is_object() && saved["remote"]["state"].is_null());
        failed.internal_state.as_mut().unwrap()["pendingImport"] = Value::Null;

        let merged = in_context(|ctx| reimported(failed, imported_zone("Z2"), ctx)).unwrap();
        assert_eq!(
            merged.internal_state.as_ref().unwrap()["pendingImport"]["zoneId"],
            "Z2"
        );
        assert_eq!(zone_id(merged.outputs.as_ref()), "Z2");
    }

    /// A manual retry resumes from the controller saved when the resource failed, so a setup
    /// rerun's import reaches that one too.
    #[tokio::test]
    async fn a_retry_after_a_setup_rerun_sends_the_new_import() {
        use crate::core::StackResourceStateExt;

        let transport = fails_first(false);
        let failed = step_zone(transport.clone(), imported_zone("Z1")).await;
        assert_eq!(failed.status, ResourceStatus::RefreshFailed);
        assert!(failed.last_failed_state.is_some());

        let mut merged = in_context(|ctx| reimported(failed, imported_zone("Z2"), ctx)).unwrap();
        assert!(merged.retry_failed().unwrap());
        assert_eq!(zone_id(merged.outputs.as_ref()), "Z2");

        step_zone(transport.clone(), merged).await;
        let imports = transport.imports.lock().unwrap();
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[1].as_ref().unwrap()["zoneId"], "Z2");
    }

    #[test]
    fn an_imported_resource_waits_on_the_proxy_with_tagged_outputs() {
        let state = import(json!({ "state": "ready", "zoneId": "Z1" })).unwrap();
        let saved = state.internal_state.unwrap();
        assert_eq!(saved["type"], "ExtensionProxyController");
        assert_eq!(saved["_controllerStateVersion"], 1);
        assert_eq!(
            saved["pendingImport"],
            json!({ "state": "ready", "zoneId": "Z1" })
        );
        assert!(saved["remote"].is_null());
        assert_eq!(state.status, ResourceStatus::Running);
        assert_eq!(state.lifecycle, Some(ResourceLifecycle::Frozen));
        assert_eq!(
            serde_json::to_value(state.outputs.unwrap()).unwrap(),
            json!({ "type": "acme/dns-zone", "zoneId": "Z1", "domain": "a.example" })
        );
    }

    /// A stack listing `acme`, whose `acme/dns-zone` link declares `declared`, and the
    /// extensions this process loaded: `acme`'s hooks module, whose link answers `answer`, or none.
    fn link_stack(declared: &[&str], answer: &str, load: bool) -> (Stack, LoadedExtensions) {
        use crate::extension_bundle::{testing::TestBundle, ExtensionBundle, Runtime};

        let bundle = TestBundle::with_hooks_answering(
            "acme",
            "1.0.0",
            r#"[{"name":"link-zone","kind":"link","resourceType":"acme/dns-zone"}]"#,
            answer,
            &[],
        );
        let mut manifest = manifest("acme", "acme/dns-zone");
        manifest.digest = bundle.manifest.digest.clone();
        manifest
            .resource_types
            .get_mut(&ResourceType::from("acme/dns-zone"))
            .unwrap()
            .link
            .env = declared.iter().map(|name| name.to_string()).collect();
        let mut stack = Stack::new("app".to_string()).build();
        stack.extensions = vec![manifest];
        let mut loaded = LoadedExtensions::default();
        if load {
            let hooks = ExtensionBundle::open(&bundle.manifest, &bundle.zip)
                .unwrap()
                .load_hooks(&Runtime::new().unwrap())
                .unwrap();
            loaded = loaded.with_hooks("acme", hooks);
        }
        (stack, loaded)
    }

    async fn link_env(
        declared: &[&str],
        answer: &str,
        load: bool,
    ) -> Result<Vec<(String, String)>> {
        let (stack, loaded) = link_stack(declared, answer, load);
        link_environment(
            &stack,
            Some(&loaded),
            &ResourceType::from("acme/dns-zone"),
            "zone",
            json!({ "zoneId": "Z1" }),
        )
        .await
    }

    #[tokio::test]
    async fn a_link_gives_the_variables_its_type_declares() {
        let env = link_env(
            &["ACME_ZONE_ID"],
            r#"{"env":{"ACME_ZONE_ID":"Z1"},"permissionSets":[]}"#,
            true,
        )
        .await
        .unwrap();
        assert_eq!(env, [("ACME_ZONE_ID".to_string(), "Z1".to_string())]);
    }

    #[tokio::test]
    async fn a_link_that_sets_an_undeclared_variable_is_refused() {
        let error = link_env(
            &["ACME_ZONE_ID"],
            r#"{"env":{"ACME_ZONE_ID":"Z1","ALIEN_MANAGER_URL":"https://elsewhere"},"permissionSets":[]}"#,
            true,
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("`ALIEN_MANAGER_URL`"), "{error:?}");
        assert!(!error.retryable);
    }

    #[tokio::test]
    async fn a_link_whose_hooks_module_is_not_loaded_fails() {
        let error = link_env(&["ACME_ZONE_ID"], "", false).await.unwrap_err();
        assert!(error.message.contains("has not loaded"), "{error:?}");
    }

    /// A type that declares no variables keeps working where its module isn't loaded, as it did
    /// before links set variables.
    #[tokio::test]
    async fn a_type_that_declares_no_variables_runs_no_link() {
        assert!(link_env(&[], "", false).await.unwrap().is_empty());
    }
}
