//! What crosses between Alien and an extension's hooks module: the hooks the module declares, and
//! the input and output of each kind of hook.
//!
//! Every value crosses as JSON in the serde forms below. The extension SDK writes them inside the
//! module and the hosts that call hooks read them, so both sides compile against these types.
//! Where a hook returns a type a format crate owns (a CloudFormation template, a resource or an
//! expression), [`HookKind`] names it and it crosses in that type's own serde form.

use crate::{
    ManagementConfig, Platform, ResourceStatus, ResourceType, ResourceTypeDescriptor, StackSettings,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// A hook's error as it crosses back to the host: the hook's code and message, and whether
/// another try may pass. The host keeps the code when it is a short `[A-Z0-9_]` token and the
/// retry flag, and cuts the message short. Only an error that sets `retryable` itself is
/// retried; one converted from an `AlienError` is final, since a hook gives the same answer to
/// the same input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl HookError {
    /// The code of an error the hook gave only as text.
    pub const UNCODED: &'static str = "HOOK_FAILED";

    /// A final error with `message` and no code of its own.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: Self::UNCODED.to_string(),
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for HookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for HookError {}

impl From<String> for HookError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for HookError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

impl<E> From<alien_error::AlienError<E>> for HookError
where
    E: alien_error::AlienErrorData + Clone + std::fmt::Debug + Serialize,
{
    fn from(error: alien_error::AlienError<E>) -> Self {
        Self {
            code: error.code.clone(),
            message: error.message.clone(),
            retryable: false,
        }
    }
}

/// The hooks a module declares, as its `alien_hooks_manifest` export returns them.
///
/// The runtime reads it once per module, at load, so a host can route each call to a hook
/// without running the module first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksManifest {
    /// The extension's name, as errors from its hooks show it
    pub extension: String,
    /// The hooks, in the order the extension registered them, which is the order modifiers run in
    pub hooks: Vec<HookEntry>,
    /// The resource types the extension declares, with what the module knows of each: every
    /// field but `controllerPlatforms`, which the controller binary lists
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub resource_types: IndexMap<ResourceType, ResourceTypeDescriptor>,
}

/// One hook the module declares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEntry {
    /// The name a host calls the hook by
    pub name: String,
    /// What the hook does, which decides its input, its output and when Alien calls it
    pub kind: HookKind,
    /// What a host needs to route calls to the hook: `resourceType` and `platform` for an
    /// emitter or an importer, `resourceType` for a link
    #[serde(flatten)]
    pub routing: Map<String, Value>,
}

/// The kinds of hook a module can declare. A module that declares any other kind is refused
/// at load, since no host could call that hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HookKind {
    /// Changes the CloudFormation template Alien generates: a `CfTemplate` in, a `CfTemplate` out
    CloudformationModifier,
    /// Changes the Terraform module Alien generates: [`TemplateFiles`] in,
    /// [`TerraformModifierOutput`] out
    TerraformModifier,
    /// Adds templates to the Helm chart Alien generates: [`TemplateFiles`] in,
    /// [`HelmModifierOutput`] out
    HelmModifier,
    /// Writes a custom resource into a CloudFormation template: [`EmitInput`] in. Out, by
    /// method: `emit`, the resources as `[logicalId, CfResource]` pairs; `emitImportRef`, a
    /// `CfExpression`; `emitBindingRef`, a `CfExpression` or `null`
    CloudformationEmitter,
    /// Writes a custom resource into a Terraform module: [`EmitInput`] in. Out, by method:
    /// `emit`, a [`TerraformFragment`]; `emitImportRef`, an HCL expression as text;
    /// `emitBindingRef`, an HCL expression as text or `null`
    TerraformEmitter,
    /// Writes a custom resource into a Helm chart: [`EmitInput`] in, [`HelmFragmentOutput`] out
    HelmEmitter,
    /// Turns what a setup template created into a custom resource's controller state:
    /// [`ImportInput`] in, [`ImportOutput`] out
    Importer,
    /// Says what a workload gets when it links a custom resource: the resource's outputs in,
    /// [`LinkOutput`] out
    Link,
    /// Changes the requests Alien's controllers send to AWS: [`InterceptorInput`] in,
    /// [`InterceptorOutput`] out
    AwsInterceptor,
    /// Changes the requests Alien's controllers send to Google Cloud
    GcpInterceptor,
    /// Changes the requests Alien's controllers send to Azure
    AzureInterceptor,
    /// Changes the requests Alien's controllers send to Kubernetes
    KubernetesInterceptor,
}

/// A Terraform module or a Helm chart, file by file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateFiles {
    /// Path to contents, in the order Alien renders them
    pub files: IndexMap<String, String>,
}

/// What a Terraform modifier returns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerraformModifierOutput {
    /// The files the modifier changed, whole, by path. Every other file stays as Alien wrote it.
    pub changed: IndexMap<String, String>,
}

/// What a Helm modifier returns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmModifierOutput {
    /// New files for the chart's `templates/` directory, by file name
    pub templates: BTreeMap<String, String>,
}

/// Which method of an emitter a call runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EmitMethod {
    /// The resources or blocks that back the resource
    Emit,
    /// The expression that resolves to the importer's input once the template is applied
    EmitImportRef,
    /// The expression that resolves to the resource's runtime binding, if it has one
    EmitBindingRef,
}

/// What an emitter gets: the parts of Alien's `EmitContext` that serialize.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmitInput {
    /// The emitter method to run
    pub method: EmitMethod,
    /// The id of the resource being emitted
    pub resource_id: String,
    /// The platform the template is for
    pub platform: Platform,
    /// Whether the template runs workloads on Kubernetes rather than the cloud's own compute
    pub targets_kubernetes: bool,
    /// The deployer's stack settings
    pub stack_settings: StackSettings,
    /// The names the generator gave each resource id in the template
    pub names: IndexMap<String, String>,
    /// The resource's entry in the stack, `ResourceEntry` in its serde form
    pub resource: Value,
}

/// The blocks a Terraform emitter returns, each list as HCL text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerraformFragment {
    /// The oldest setup reader that understands the fragment's import payload
    pub minimum_setup_import_format_version: u32,
    /// `variable` blocks
    pub variable_blocks: String,
    /// `moved` blocks
    pub moved_blocks: String,
    /// `resource` blocks
    pub resource_blocks: String,
    /// `data` blocks
    pub data_blocks: String,
    /// `ephemeral` blocks
    pub ephemeral_blocks: String,
    /// Entries for the module's `locals`, each an HCL expression as text
    pub locals: IndexMap<String, String>,
}

/// What a Helm emitter returns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmFragmentOutput {
    /// The resource's entry under `infrastructure:` in `values.yaml`, if it has one
    pub infrastructure: Option<HelmInfrastructureValue>,
    /// Extra files for the chart's `templates/` directory, by path
    pub extra_templates: IndexMap<String, String>,
}

/// One entry under `infrastructure:` in a chart's `values.yaml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelmInfrastructureValue {
    /// The resource id, the entry's key
    pub id: String,
    /// The runtime binding family
    pub binding_type: String,
    /// The service the resource binds to
    pub service: String,
    /// The entry's fields, in the order the emitter wrote them
    pub fields: IndexMap<String, String>,
}

/// What an importer gets: the parts of Alien's `ImportContext` that serialize, and the data
/// the setup template returned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportInput {
    /// The id of the resource being imported
    pub resource_id: String,
    /// The platform the resource was set up on
    pub platform: Platform,
    /// The region or location the setup template reported
    pub region: String,
    /// The stack settings the setup template reported
    pub stack_settings: StackSettings,
    /// The management configuration, when setup created a management identity
    pub management_config: Option<ManagementConfig>,
    /// The resource's entry in the stack, `ResourceEntry` in its serde form
    pub resource: Value,
    /// What the emitter's import reference resolved to after setup
    pub import_data: Value,
}

/// What an importer returns. The controller state carries no type tag or state version, which
/// only Alien writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutput {
    /// The controller's saved fields and its state name
    pub internal_state: Value,
    /// The resource's status after the import
    pub status: ResourceStatus,
    /// The resource's outputs, without a type tag
    pub outputs: Option<Value>,
}

/// What a link returns for a workload that links the resource.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkOutput {
    /// Environment variables the workload gets, in the order the link set them
    pub env: IndexMap<String, String>,
    /// Ids of the permission sets the workload gets on the resource
    pub permission_sets: Vec<String>,
}

/// A call to an interceptor, by what it asks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum InterceptorInput {
    /// Whether the interceptor sees this call. Answered by [`InterceptorOutput::Matches`].
    Matches {
        /// The call
        request: InterceptedRequest,
    },
    /// Change the request. Answered by [`InterceptorOutput::Request`].
    ModifyRequest {
        /// The request as Alien built it and earlier interceptors left it
        request: InterceptedRequest,
    },
    /// Change the response. Answered by [`InterceptorOutput::Response`].
    ModifyResponse {
        /// The request as it was sent
        request: InterceptedRequest,
        /// The response as the cloud sent it and earlier interceptors left it
        response: InterceptedResponse,
    },
}

/// An interceptor's answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterceptorOutput {
    /// Whether the interceptor sees the call
    Matches(bool),
    /// The request to send
    Request(InterceptedRequest),
    /// The response Alien reads
    Response(InterceptedResponse),
}

/// The API a call goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InterceptedCloud {
    Aws,
    Gcp,
    Azure,
    Kubernetes,
}

/// A request as an interceptor sees it: no credential headers, and a text body or none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterceptedRequest {
    /// The API the call goes to
    pub cloud: InterceptedCloud,
    /// The service the call is signed for: the SigV4 signing name on AWS (`rds`), Alien's
    /// endpoint key on Google Cloud (`cloudsql`), `management` for Azure Resource Manager, and
    /// `kubernetes` for the API server
    pub service: String,
    /// The HTTP method
    pub method: String,
    /// The full URL
    pub url: String,
    /// The headers, in order, a name once per value
    pub headers: Vec<(String, String)>,
    /// The body as text, or `None` when the request has none
    pub body: Option<String>,
    /// The Azure tenant the call is made in, on Azure
    pub tenant_id: Option<String>,
}

/// A response as an interceptor sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterceptedResponse {
    /// The HTTP status
    pub status: u16,
    /// The headers, in order, a name once per value
    pub headers: Vec<(String, String)>,
    /// The body as text
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_alien_error_crosses_as_final() {
        let error = alien_error::AlienError::new(crate::error::ErrorData::GenericError {
            message: "the zone already exists".to_string(),
        });
        assert!(error.retryable);
        let error = HookError::from(error);
        assert!(
            !error.retryable,
            "a hook gives the same answer to the same input"
        );
        assert_eq!(error.code, "GENERIC_ERROR");
    }
}
