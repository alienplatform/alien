//! A deployment's registries: Alien's built-ins, then the embedding program's registrations, then the
//! extensions its stack lists, in the order the stack lists them.
//!
//! Every registry here is a value built for one stack. Two deployments with different extensions
//! build two sets, so neither sees the other's emitters, importers or controllers.
//!
//! A custom type's emitters and importers run the extension's hooks module. Its controller is the
//! step-protocol proxy, reached through the endpoint the process loaded for the extension.

use crate::core::{serialize_controller, PlatformServiceProvider, ResourceRegistry};
use crate::import::ImporterRegistry;
use crate::modifiers::Modifiers;
use crate::refusal::RefusalContext;
use crate::step_protocol::{ExtensionProxyController, ProxyFactory};
use alien_cloudformation::{CfEmitter, CfExpression, CfRegistry, CfResource};
use alien_core::hooks::{
    EmitInput, EmitMethod, HelmFragmentOutput, HookKind, ImportInput, ImportOutput,
    TerraformFragment,
};
use alien_core::import::{EmitContext, ImportContext, ResourceImporter};
use alien_core::{
    CustomResourceOutputs, ErrorData, Platform, ResourceOutputs, ResourceType, Result, Stack,
    StackResourceState,
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
    /// The template modifiers each generator call site runs on what the generator returns.
    pub modifiers: Modifiers,
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

        let mut modifiers = Vec::with_capacity(stack.extensions.len());
        for manifest in &stack.extensions {
            let hooks = Arc::new(ExtensionHooks {
                extension: manifest.name.clone(),
                module: extensions.hooks.get(&manifest.name).cloned(),
            });
            modifiers.push(hooks.clone());
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
            modifiers: Modifiers::new(modifiers),
        }
    }
}

impl ResourceRegistry {
    /// Alien's built-in controllers, the host's, and the step-protocol proxy for each custom
    /// type and platform an extension in `stack` ships a controller for and `extensions` has an
    /// endpoint for.
    pub fn with_built_ins_and_extensions(stack: &Stack, extensions: &LoadedExtensions) -> Self {
        let mut registry = Self::with_built_ins();
        registry.add_extensions(stack, extensions, |_| true);
        registry
    }

    /// The registry of a deployment moving from the `current` release to the `target` one, each
    /// a stack with what this process loaded for its extensions. A type the target stack defines
    /// gets the target release's controller; a type only the current stack defines, which the
    /// update removes, keeps the current release's, so its resources can still be deleted.
    pub fn with_built_ins_and_releases(
        target: (&Stack, &LoadedExtensions),
        current: (&Stack, &LoadedExtensions),
    ) -> Self {
        let mut registry = Self::with_built_ins();
        registry.add_extensions(target.0, target.1, |_| true);
        registry.add_extensions(current.0, current.1, |resource_type| {
            target.0.descriptor(resource_type).is_none()
        });
        registry
    }

    /// Registers the controllers of `stack`'s extensions for the types `include` accepts.
    fn add_extensions(
        &mut self,
        stack: &Stack,
        extensions: &LoadedExtensions,
        include: impl Fn(&ResourceType) -> bool,
    ) {
        for manifest in &stack.extensions {
            let factory = extensions.controllers.get(&manifest.name);
            for (resource_type, descriptor) in declared_types(stack, manifest) {
                if !include(resource_type) {
                    continue;
                }
                for &platform in &descriptor.controller_platforms {
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
    /// An extension whose module this process hasn't loaded.
    #[cfg(test)]
    pub(crate) fn unloaded(extension: &str) -> Self {
        Self {
            extension: extension.to_string(),
            module: None,
        }
    }

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
            stack_settings: ctx.stack_settings.clone(),
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
}

/// The resource state an importer's output describes: its fields wait on the proxy until the
/// controller loads them, and its outputs carry the type tag.
fn imported_state(
    resource_type: &ResourceType,
    imported: ImportOutput,
    ctx: &ImportContext<'_>,
) -> Result<StackResourceState> {
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
        let entry = entry();
        let settings = StackSettings::default();
        imported_state(
            &ResourceType::from("acme/dns-zone"),
            ImportOutput {
                internal_state,
                status: ResourceStatus::Running,
                outputs: Some(json!({ "zoneId": "Z1", "domain": "a.example" })),
            },
            &ImportContext {
                resource_id: "zone",
                platform: Platform::Aws,
                region: "us-east-1",
                stack_settings: &settings,
                management_config: None,
                resource: &entry,
            },
        )
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
        let mut manifest = manifest("acme", types[0]);
        for resource_type in types {
            let mut descriptor = manifest.resource_types[0].clone();
            descriptor.controller_platforms = vec![Platform::Aws];
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
}
