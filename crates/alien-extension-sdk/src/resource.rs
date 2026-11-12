//! Custom resource types: their registration, and the module side of their emitters, importers
//! and links.

use crate::{read_input, write_output, Extension};
use alien_cloudformation::CfEmitter;
use alien_core::hooks::HookError;
use alien_core::hooks::{
    EmitInput, EmitMethod, HelmFragmentOutput, HelmInfrastructureValue, HookKind, ImportInput,
    ImportOutput, LinkOutput, TerraformFragment,
};
use alien_core::import::{EmitContext, ImportContext, ResourceImporter};
use alien_core::{
    ErrorData, Platform, Resource, ResourceDefinition, ResourceEntry, ResourceLifecycle,
    ResourceOutputs, ResourceOutputsDefinition, ResourceRef, ResourceStatus, ResourceType,
    ResourceTypeDescriptor, Result, Stack, StackResourceState,
};
use alien_error::AlienError;
use alien_helm::HelmEmitter;
use alien_terraform::{TfEmitter, TfFragment};
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::any::TypeId;
use std::marker::PhantomData;

use crate::descriptor;

/// A custom resource type: its config struct, and the outputs a linked workload sees.
/// `#[alien_resource]` implements it.
pub trait AlienResource:
    ResourceDefinition + Serialize + DeserializeOwned + Clone + utoipa::ToSchema + 'static
{
    /// The type name, `<extension>/<type>`, such as `acme/dns-zone`.
    const RESOURCE_TYPE: ResourceTypeOf<Self>;

    /// The lifecycles a resource of this type may have.
    const LIFECYCLES: &'static [ResourceLifecycle];

    /// Whether deleting the resource can lose the customer's data.
    const HOLDS_DATA: bool;

    /// What a linked workload and the resource's link function see.
    type Outputs: ResourceOutputsDefinition
        + Serialize
        + DeserializeOwned
        + Clone
        + utoipa::ToSchema
        + 'static;
}

/// A resource type's name that also names its config struct `T`, so code given the name, such as
/// `run_controllers`' `register`, reads a resource's config as `T`. `#[alien_resource]` writes
/// one as the struct's `RESOURCE_TYPE`; `into()` or `resource_type()` gives the plain name.
pub struct ResourceTypeOf<T> {
    resource_type: ResourceType,
    config: PhantomData<fn() -> T>,
}

impl<T> ResourceTypeOf<T> {
    pub const fn new(resource_type: ResourceType) -> Self {
        Self {
            resource_type,
            config: PhantomData,
        }
    }

    /// The type's name, such as `acme/dns-zone`.
    pub fn resource_type(&self) -> ResourceType {
        self.resource_type.clone()
    }
}

impl<T> Clone for ResourceTypeOf<T> {
    fn clone(&self) -> Self {
        Self::new(self.resource_type.clone())
    }
}

impl<T> From<ResourceTypeOf<T>> for ResourceType {
    fn from(typed: ResourceTypeOf<T>) -> Self {
        typed.resource_type
    }
}

impl<T> std::fmt::Display for ResourceTypeOf<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.resource_type.fmt(f)
    }
}

impl<T> std::fmt::Debug for ResourceTypeOf<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.resource_type.fmt(f)
    }
}

/// The config of the resource a CloudFormation emitter emits, as `T`. Alien's own helper, taking
/// the type's `RESOURCE_TYPE` as `#[alien_resource]` writes it.
pub fn resource_config<'a, T: ResourceDefinition>(
    ctx: &'a EmitContext<'_>,
    expected: impl Into<ResourceType>,
) -> Result<&'a T> {
    alien_cloudformation::emitters::aws::helpers::resource_config(ctx, expected.into())
}

/// The config of the resource a Terraform emitter emits, as `T`. Alien's own helper, taking the
/// type's `RESOURCE_TYPE` as `#[alien_resource]` writes it.
pub fn downcast<'a, T: ResourceDefinition>(
    ctx: &'a EmitContext<'_>,
    expected: impl Into<ResourceType>,
) -> Result<&'a T> {
    alien_terraform::emitters::gcp::helpers::downcast(ctx, expected.into())
}

/// Adds the pieces of one custom resource type to an [`Extension`], from
/// [`Extension::resource`].
pub struct ResourceRegistration<'a, T> {
    extension: &'a mut Extension,
    resource: PhantomData<fn() -> T>,
}

impl<'a, T: AlienResource> ResourceRegistration<'a, T> {
    pub(crate) fn new(extension: &'a mut Extension) -> Self {
        Self {
            extension,
            resource: PhantomData,
        }
    }

    /// A permission set, in the JSONC format of custom permission sets, one per call.
    pub fn permission_set(self, jsonc: &str) -> Self {
        self.extension
            .permission_sets
            .push((T::RESOURCE_TYPE.resource_type(), jsonc.to_string()));
        self
    }

    /// The resource's part of the CloudFormation template on `platform`.
    pub fn cloudformation_emitter(
        mut self,
        platform: Platform,
        emitter: impl CfEmitter + 'static,
    ) -> Self {
        self.declarations().setup_cloudformation.push(platform);
        self.add(
            HookKind::CloudformationEmitter,
            Some(platform),
            move |input| emit_cloudformation::<T>(&emitter, input),
        )
    }

    /// The resource's part of the Terraform module on `platform`.
    pub fn terraform_emitter(
        mut self,
        platform: Platform,
        emitter: impl TfEmitter + 'static,
    ) -> Self {
        self.declarations().setup_terraform.push(platform);
        self.add(HookKind::TerraformEmitter, Some(platform), move |input| {
            emit_terraform::<T>(&emitter, input)
        })
    }

    /// The resource's part of the Helm chart on `platform`.
    pub fn helm_emitter(mut self, platform: Platform, emitter: impl HelmEmitter + 'static) -> Self {
        self.declarations().setup_helm.push(platform);
        self.add(HookKind::HelmEmitter, Some(platform), move |input| {
            emit_helm::<T>(&emitter, input)
        })
    }

    /// The importer that turns the setup template's outputs on `platform` into the controller's
    /// saved fields.
    pub fn importer<I: ResourceImporter + 'static>(
        mut self,
        platform: Platform,
        importer: I,
    ) -> Self {
        self.declarations().setup_importers.push(platform);
        self.add(HookKind::Importer, Some(platform), move |input| {
            import::<T, I>(&importer, input)
        })
    }

    /// The cloud services the type needs on `platform`: GCP APIs such as `dns.googleapis.com`,
    /// or Azure resource providers such as `Microsoft.Network`. Alien enables them with the ones
    /// its own resource types need, once each.
    pub fn requires_services<S: Into<String>>(
        mut self,
        platform: Platform,
        services: impl IntoIterator<Item = S>,
    ) -> Self {
        for service in services {
            let service = service.into();
            if !alien_core::is_cloud_service_name(platform, &service) {
                self.extension.problems.push(format!(
                    "`{}` requires the service `{service}` on {platform}, which is not {}",
                    T::RESOURCE_TYPE,
                    match platform {
                        Platform::Gcp => "a GCP API name, `<name>.googleapis.com`",
                        Platform::Azure =>
                            "an Azure resource provider, `Microsoft.<Namespace>`",
                        _ => "a GCP API or an Azure resource provider: Alien enables services only on GCP and Azure",
                    }
                ));
                continue;
            }
            let declared = self.declarations().services.entry(platform).or_default();
            if !declared
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&service))
            {
                declared.push(service);
            }
        }
        self
    }

    /// The type needs the stack's network, so Alien creates it on AWS, GCP and Azure.
    pub fn requires_network(mut self) -> Self {
        self.declarations().requires_network = true;
        self
    }

    /// What a workload gets when it links the resource, from the resource's outputs. The
    /// permission sets it grants go into the type's descriptor: setup grants them before the
    /// resource has outputs, so `register` runs `link` once on placeholder outputs to read them.
    pub fn link(mut self, link: impl Fn(&T::Outputs) -> Link + 'static) -> Self {
        match descriptor::placeholder::<T::Outputs>() {
            Ok(outputs) => {
                self.declarations().link_permission_sets = Some(link(&outputs).permission_sets);
            }
            Err(error) => self.extension.problems.push(format!(
                "`{}`'s link can't run on placeholder outputs to read its permission sets: {error}",
                T::RESOURCE_TYPE
            )),
        }
        self.add(HookKind::Link, None, move |input| {
            let outputs: T::Outputs = read_input(input, "resource outputs")?;
            let link = link(&outputs);
            write_output(&LinkOutput {
                env: link.env,
                permission_sets: link.permission_sets,
            })
        })
    }

    fn declarations(&mut self) -> &mut ResourceDeclarations {
        self.extension
            .declarations
            .entry(T::RESOURCE_TYPE.resource_type())
            .or_insert_with(ResourceDeclarations::new::<T>)
    }

    /// Adds a hook for this type, named after its kind, the type and the platform, which a host
    /// routes calls by.
    fn add(
        self,
        kind: HookKind,
        platform: Option<Platform>,
        run: impl Fn(&[u8]) -> std::result::Result<Vec<u8>, HookError> + 'static,
    ) -> Self {
        let mut routing = Map::new();
        let mut name = format!("{}/{}", crate::kind_name(kind), T::RESOURCE_TYPE);
        routing.insert("resourceType".into(), T::RESOURCE_TYPE.to_string().into());
        if let Some(platform) = platform {
            let platform = match serde_json::to_value(platform) {
                Ok(Value::String(platform)) => platform,
                _ => unreachable!("a platform serializes as its name"),
            };
            name = format!("{name}/{platform}");
            routing.insert("platform".into(), platform.into());
        }
        self.extension.add(name, kind, routing, Box::new(run));
        self
    }
}

/// What `register` declared about one resource type, which its descriptor is built from.
pub(crate) struct ResourceDeclarations {
    pub(crate) lifecycles: Vec<ResourceLifecycle>,
    pub(crate) holds_data: bool,
    pub(crate) config_schema: Map<String, Value>,
    pub(crate) outputs_schema: Map<String, Value>,
    pub(crate) setup_cloudformation: Vec<Platform>,
    pub(crate) setup_terraform: Vec<Platform>,
    pub(crate) setup_helm: Vec<Platform>,
    pub(crate) setup_importers: Vec<Platform>,
    /// `None` when the type has no link.
    pub(crate) link_permission_sets: Option<Vec<String>>,
    services: IndexMap<Platform, Vec<String>>,
    requires_network: bool,
}

impl ResourceDeclarations {
    pub(crate) fn new<T: AlienResource>() -> Self {
        Self {
            lifecycles: T::LIFECYCLES.to_vec(),
            holds_data: T::HOLDS_DATA,
            config_schema: descriptor::schema::<T>(),
            outputs_schema: descriptor::schema::<T::Outputs>(),
            setup_cloudformation: Vec::new(),
            setup_terraform: Vec::new(),
            setup_helm: Vec::new(),
            setup_importers: Vec::new(),
            link_permission_sets: None,
            services: IndexMap::new(),
            requires_network: false,
        }
    }

    pub(crate) fn write(&self, descriptor: &mut ResourceTypeDescriptor) {
        descriptor.services = self.services.clone();
        descriptor.requires_network = self.requires_network;
    }
}

/// What a workload gets when it links a custom resource.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    env: IndexMap<String, String>,
    permission_sets: Vec<String>,
}

impl Link {
    /// An empty link.
    pub fn new() -> Self {
        Self::default()
    }

    /// An environment variable the workload gets.
    pub fn env(mut self, name: &str, value: impl Into<String>) -> Self {
        self.env.insert(name.to_string(), value.into());
        self
    }

    /// A permission set the workload gets on the resource, by id.
    pub fn permission_set(mut self, id: &str) -> Self {
        self.permission_sets.push(id.to_string());
        self
    }
}

/// Builds an importer's result.
pub trait ImportContextExt {
    /// The imported resource, running, with its controller at `Ready` holding `fields`, and
    /// `outputs`. Alien adds the controller state's type tag and version. A type with no
    /// controller passes `()`, which saves no fields.
    fn ready<F: Serialize + 'static, O: ResourceOutputsDefinition + 'static>(
        &self,
        fields: F,
        outputs: O,
    ) -> Result<StackResourceState>;
}

impl ImportContextExt for ImportContext<'_> {
    fn ready<F: Serialize + 'static, O: ResourceOutputsDefinition + 'static>(
        &self,
        fields: F,
        outputs: O,
    ) -> Result<StackResourceState> {
        let mut state = if TypeId::of::<F>() == TypeId::of::<()>() {
            Value::Object(Map::new())
        } else {
            serde_json::to_value(fields).map_err(|error| {
                AlienError::new(ErrorData::JsonSerializationFailed {
                    reason: error.to_string(),
                })
            })?
        };
        let Value::Object(fields) = &mut state else {
            return Err(AlienError::new(ErrorData::GenericError {
                message: "the controller's fields must serialize as a JSON object".to_string(),
            }));
        };
        fields.insert("state".into(), "ready".into());
        Ok(StackResourceState::builder()
            .resource_type(self.resource.config.resource_type().0.into_owned())
            .status(ResourceStatus::Running)
            .config(self.resource.config.clone())
            .internal_state(state)
            .outputs(ResourceOutputs::new(outputs))
            .lifecycle(self.resource.lifecycle)
            .dependencies(self.resource.combined_dependencies())
            .build())
    }
}

/// A resource entry's fields besides its config, in `ResourceEntry`'s serde form.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EntryFields {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    lifecycle: ResourceLifecycle,
    dependencies: Vec<ResourceRef>,
    #[serde(default)]
    remote_access: bool,
    #[serde(default)]
    enabled_when: Option<String>,
}

/// Reads a stack entry of the extension's own type. Alien's `Resource` deserializer knows only
/// Alien's types, and the hook knows `T`.
fn resource_entry<T: AlienResource>(value: Value) -> std::result::Result<ResourceEntry, String> {
    let Value::Object(mut entry) = value else {
        return Err("the resource entry is not a JSON object".to_string());
    };
    let Some(Value::Object(mut config)) = entry.remove("config") else {
        return Err("the resource entry has no config".to_string());
    };
    let resource_type = config.remove("type");
    if resource_type.as_ref().and_then(Value::as_str)
        != Some(T::RESOURCE_TYPE.resource_type().0.as_ref())
    {
        return Err(format!(
            "this hook handles {}, and Alien sent {}",
            T::RESOURCE_TYPE,
            resource_type.unwrap_or(Value::Null)
        ));
    }
    let config: T = serde_json::from_value(Value::Object(config))
        .map_err(|error| format!("the {} config did not read: {error}", T::RESOURCE_TYPE))?;
    let fields: EntryFields = serde_json::from_value(Value::Object(entry))
        .map_err(|error| format!("the resource entry did not read: {error}"))?;
    Ok(ResourceEntry {
        display_name: fields.display_name,
        description: fields.description,
        config: Resource::new(config),
        lifecycle: fields.lifecycle,
        dependencies: fields.dependencies,
        remote_access: fields.remote_access,
        enabled_when: fields.enabled_when,
    })
}

/// Runs `emit` with the `EmitContext` an [`EmitInput`] describes. The context's stack holds only
/// the resource being emitted.
fn with_emit_context<T: AlienResource, R>(
    input: &[u8],
    emit: impl FnOnce(EmitMethod, &EmitContext<'_>) -> std::result::Result<R, HookError>,
) -> std::result::Result<R, HookError> {
    let input: EmitInput = read_input(input, "emitter input")?;
    let entry = resource_entry::<T>(input.resource)?;
    let mut stack = Stack::new("extension".to_string()).build();
    stack
        .resources
        .insert(input.resource_id.clone(), entry.clone());
    let ctx = EmitContext {
        stack: &stack,
        resource: &entry,
        resource_id: &input.resource_id,
        platform: input.platform,
        targets_kubernetes: input.targets_kubernetes,
        stack_settings: &input.stack_settings,
        names: &input.names,
    };
    emit(input.method, &ctx)
}

fn emit_cloudformation<T: AlienResource>(
    emitter: &dyn CfEmitter,
    input: &[u8],
) -> std::result::Result<Vec<u8>, HookError> {
    with_emit_context::<T, _>(input, |method, ctx| match method {
        EmitMethod::Emit => {
            let resources = emitter.emit_resources(ctx).map_err(HookError::from)?;
            let resources: Vec<_> = resources
                .into_iter()
                .map(|resource| (resource.logical_id.clone(), resource))
                .collect();
            write_output(&resources)
        }
        EmitMethod::EmitImportRef => {
            write_output(&emitter.emit_import_ref(ctx).map_err(HookError::from)?)
        }
        EmitMethod::EmitBindingRef => {
            write_output(&emitter.emit_binding_ref(ctx).map_err(HookError::from)?)
        }
    })
}

fn emit_terraform<T: AlienResource>(
    emitter: &dyn TfEmitter,
    input: &[u8],
) -> std::result::Result<Vec<u8>, HookError> {
    with_emit_context::<T, _>(input, |method, ctx| match method {
        EmitMethod::Emit => {
            let fragment = emitter.emit(ctx).map_err(HookError::from)?;
            write_output(&terraform_fragment(fragment)?)
        }
        EmitMethod::EmitImportRef => {
            let expression = emitter.emit_import_ref(ctx).map_err(HookError::from)?;
            write_output(&hcl_text(&expression)?)
        }
        EmitMethod::EmitBindingRef => {
            let expression = emitter.emit_binding_ref(ctx).map_err(HookError::from)?;
            write_output(&expression.as_ref().map(hcl_text).transpose()?)
        }
    })
}

/// A fragment's blocks as HCL text. Marking blocks shared, gated or retained is for Alien's own
/// emitters, whose blocks the generator deduplicates or gates; an extension's blocks are its own.
fn terraform_fragment(fragment: TfFragment) -> std::result::Result<TerraformFragment, String> {
    let marked = fragment.resource_blocks.iter().any(|block| {
        fragment.is_shared(block)
            || fragment.is_retained(block)
            || fragment.contribution_gates(block).is_some()
    });
    if marked {
        return Err(
            "an extension's Terraform emitter can't mark blocks shared, gated or retained"
                .to_string(),
        );
    }
    let blocks = |blocks: Vec<hcl::Block>| hcl_text(&hcl::Body::from_iter(blocks));
    Ok(TerraformFragment {
        minimum_setup_import_format_version: fragment.minimum_setup_import_format_version,
        variable_blocks: blocks(fragment.variable_blocks)?,
        moved_blocks: blocks(fragment.moved_blocks)?,
        resource_blocks: blocks(fragment.resource_blocks)?,
        data_blocks: blocks(fragment.data_blocks)?,
        ephemeral_blocks: blocks(fragment.ephemeral_blocks)?,
        locals: fragment
            .locals
            .iter()
            .map(|(name, expression)| Ok((name.clone(), hcl_text(expression)?)))
            .collect::<std::result::Result<_, String>>()?,
    })
}

fn hcl_text(value: &impl hcl::format::Format) -> std::result::Result<String, String> {
    hcl::format::to_string(value).map_err(|error| format!("the HCL did not format: {error}"))
}

fn emit_helm<T: AlienResource>(
    emitter: &dyn HelmEmitter,
    input: &[u8],
) -> std::result::Result<Vec<u8>, HookError> {
    with_emit_context::<T, _>(input, |_, ctx| {
        let fragment = emitter.emit(ctx).map_err(HookError::from)?;
        write_output(&HelmFragmentOutput {
            infrastructure: fragment
                .infrastructure
                .map(|value| HelmInfrastructureValue {
                    id: value.id,
                    binding_type: value.binding_type,
                    service: value.service,
                    fields: value.fields,
                }),
            extra_templates: fragment.extra_templates,
        })
    })
}

fn import<T: AlienResource, I: ResourceImporter>(
    importer: &I,
    input: &[u8],
) -> std::result::Result<Vec<u8>, HookError> {
    let input: ImportInput = read_input(input, "importer input")?;
    let entry = resource_entry::<T>(input.resource)?;
    let ctx = ImportContext {
        resource_id: &input.resource_id,
        platform: input.platform,
        region: &input.region,
        stack_settings: &input.stack_settings,
        management_config: input.management_config.as_ref(),
        resource: &entry,
    };
    let data: I::ImportData = serde_json::from_value(input.import_data)
        .map_err(|error| format!("the import data did not read: {error}"))?;
    let state = importer.import(data, &ctx).map_err(HookError::from)?;
    let outputs = state
        .outputs
        .map(|outputs| {
            let mut value = serde_json::to_value(outputs)
                .map_err(|error| format!("the outputs did not serialize: {error}"))?;
            if let Value::Object(fields) = &mut value {
                fields.shift_remove("type");
            }
            Ok::<_, String>(value)
        })
        .transpose()?;
    write_output(&ImportOutput {
        internal_state: state.internal_state.unwrap_or(Value::Null),
        status: state.status,
        outputs,
    })
}
