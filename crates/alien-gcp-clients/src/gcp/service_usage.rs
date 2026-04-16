use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::longrunning::Operation;
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

/// Service Usage service configuration
#[derive(Debug)]
pub struct ServiceUsageServiceConfig;

impl GcpServiceConfig for ServiceUsageServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://serviceusage.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://serviceusage.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Service Usage"
    }

    fn service_key(&self) -> &'static str {
        "serviceusage"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ServiceUsageApi: Send + Sync + Debug {
    async fn enable_service(&self, service_name: String) -> Result<Operation>;

    async fn disable_service(
        &self,
        service_name: String,
        disable_dependent_services: Option<bool>,
        check_if_service_has_usage: Option<CheckIfServiceHasUsage>,
    ) -> Result<Operation>;

    async fn get_service(&self, service_name: String) -> Result<Service>;

    async fn get_operation(&self, operation_name: String) -> Result<Operation>;
}

/// Service Usage client for managing GCP service enablement
#[derive(Debug)]
pub struct ServiceUsageClient {
    base: GcpClientBase,
    project_id: String,
}

impl ServiceUsageClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(ServiceUsageServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ServiceUsageApi for ServiceUsageClient {
    /// Enables a service so that it can be used with a project.
    /// See: https://cloud.google.com/service-usage/docs/reference/rest/v1/services/enable
    async fn enable_service(&self, service_name: String) -> Result<Operation> {
        let path = format!(
            "projects/{}/services/{}:enable",
            self.project_id, service_name
        );

        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Option::<()>::None, // Empty request body as per API docs
                &service_name,
            )
            .await
    }

    /// Disables a service so that it can no longer be used with a project.
    /// See: https://cloud.google.com/service-usage/docs/reference/rest/v1/services/disable
    async fn disable_service(
        &self,
        service_name: String,
        disable_dependent_services: Option<bool>,
        check_if_service_has_usage: Option<CheckIfServiceHasUsage>,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/services/{}:disable",
            self.project_id, service_name
        );

        let request_body =
            if disable_dependent_services.is_some() || check_if_service_has_usage.is_some() {
                Some(DisableServiceRequest {
                    disable_dependent_services,
                    check_if_service_has_usage,
                })
            } else {
                None
            };

        self.base
            .execute_request(Method::POST, &path, None, request_body, &service_name)
            .await
    }

    /// Returns the service configuration and enabled state for a given service.
    /// See: https://cloud.google.com/service-usage/docs/reference/rest/v1/services/get
    async fn get_service(&self, service_name: String) -> Result<Service> {
        let path = format!("projects/{}/services/{}", self.project_id, service_name);

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &service_name)
            .await
    }

    /// Gets information about a long-running operation.
    /// See: https://cloud.google.com/service-usage/docs/reference/rest/v1/operations/get
    async fn get_operation(&self, operation_name: String) -> Result<Operation> {
        // GCP returns operation names that already include the "operations/" prefix
        // so we need to handle this correctly
        let path = if operation_name.starts_with("operations/") {
            operation_name.clone()
        } else {
            format!("operations/{}", operation_name)
        };

        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &operation_name,
            )
            .await
    }
}

// --- Data Structures ---

/// Request message for disabling a service.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DisableServiceRequest {
    /// Indicates if services that are enabled and which depend on this service should also be disabled.
    /// If not set, an error will be generated if any enabled services depend on the service to be disabled.
    /// When set, the service, and any enabled services that depend on it, will be disabled together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_dependent_services: Option<bool>,

    /// Defines the behavior for checking service usage when disabling a service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_if_service_has_usage: Option<CheckIfServiceHasUsage>,
}

/// Enum to determine if service usage should be checked when disabling a service.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckIfServiceHasUsage {
    /// When unset, the default behavior is used, which is SKIP.
    CheckIfServiceHasUsageUnspecified,
    /// If set, skip checking service usage when disabling a service.
    Skip,
    /// If set, service usage is checked when disabling the service.
    /// If a service, or its dependents, has usage in the last 30 days,
    /// the request returns a FAILED_PRECONDITION error.
    Check,
}

/// A service that is available for use and can be activated.
/// Based on: https://cloud.google.com/service-usage/docs/reference/rest/v1/services#Service
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    /// The resource name of the consumer and service.
    /// Example: projects/123/services/serviceusage.googleapis.com
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The resource name of the consumer.
    /// Example: projects/123
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,

    /// The service configuration of the available service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<ServiceConfig>,

    /// Whether or not the service has been enabled for use by the consumer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<State>,
}

/// The configuration of the service.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConfig {
    /// The DNS address at which this service is available.
    /// Example: serviceusage.googleapis.com
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The product title for this service.
    /// Example: Service Usage API
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// A list of API interfaces exported by this service.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apis: Vec<Api>,

    /// Additional API documentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<Documentation>,

    /// Quota configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota: Option<Quota>,

    /// Auth configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<Authentication>,

    /// Configuration controlling usage of this service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,

    /// Configuration for network endpoints.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoints: Vec<Endpoint>,

    /// Defines the monitored resources used by this service.
    /// This is required by the Service.monitoring and Service.logging configurations.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub monitored_resources: Vec<MonitoredResourceDescriptor>,

    /// Monitoring configuration. This should not include the 'producerDestinations' field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitoring: Option<Monitoring>,

    /// A list of all proto message types included in this API service.
    /// Note: This field is not part of the official ServiceConfig API but may be needed for broader proto support.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<Type>,

    /// A list of all enum types included in this API service.
    /// Note: This field is not part of the official ServiceConfig API but may be needed for broader proto support.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enums: Vec<Enum>,

    /// The semantic version of the service configuration.
    /// Note: This field is not part of the official ServiceConfig API but may be needed for broader proto support.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_version: Option<u32>,
}

/// Whether or not the service has been enabled for use by the consumer.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum State {
    /// The default value, which indicates that the enabled state of the service is unspecified or not meaningful.
    StateUnspecified,
    /// The service cannot be used by this consumer. It has either been explicitly disabled, or has never been enabled.
    Disabled,
    /// The service has been explicitly enabled for use by this consumer.
    Enabled,
}

/// An object that describes the schema of a MonitoredResource object using a type name and a set of labels.
/// Based on: https://cloud.google.com/monitoring/api/ref_v3/rest/v3/projects.monitoredResourceDescriptors
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MonitoredResourceDescriptor {
    /// The resource name of the monitored resource descriptor.
    /// Example: "projects/{project_id}/monitoredResourceDescriptors/{type}"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The monitored resource type.
    /// Example: "gce_instance"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// A concise name for the monitored resource type that might be displayed in user interfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// A detailed description of the monitored resource type that might be used in documentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// A set of labels used to describe instances of this monitored resource type.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<LabelDescriptor>,

    /// The launch stage of the monitored resource definition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_stage: Option<LaunchStage>,
}

/// A description of a label.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct LabelDescriptor {
    /// The label key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    /// The type of data that can be assigned to the label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_type: Option<ValueType>,

    /// A human-readable description for the label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Value types that can be used as label values.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ValueType {
    /// A variable-length string, in UTF-8.
    String,
    /// Boolean; true or false.
    Bool,
    /// A 64-bit signed integer.
    Int64,
}

/// The launch stage as defined by Google Cloud Platform Launch Stages.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LaunchStage {
    /// Do not use this default value.
    LaunchStageUnspecified,
    /// The feature is not yet implemented.
    Unimplemented,
    /// Prelaunch features are hidden from users and are only visible internally.
    Prelaunch,
    /// Early Access features are limited to a closed group of testers.
    EarlyAccess,
    /// Alpha is a limited availability test for releases before they are cleared for widespread use.
    Alpha,
    /// Beta is the testing stage after alpha.
    Beta,
    /// GA is the release candidate stage, the feature can be used by any customer.
    Ga,
    /// Deprecated features are scheduled to be shut down and removed.
    Deprecated,
}

/// Monitoring configuration of the service.
/// Based on: https://cloud.google.com/service-usage/docs/reference/rest/Shared.Types/Monitoring
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Monitoring {
    /// Monitoring configurations for sending metrics to the producer project.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub producer_destinations: Vec<MonitoringDestination>,

    /// Monitoring configurations for sending metrics to the consumer project.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub consumer_destinations: Vec<MonitoringDestination>,
}

/// Configuration of a specific monitoring destination (the producer project or the consumer project).
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MonitoringDestination {
    /// The hostname of the monitoring service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitored_resource: Option<String>,

    /// Types of the metrics to report to this monitoring destination.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<String>,
}

/// Api is a light-weight descriptor for an API Interface.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Api {
    /// The fully qualified name of this interface, including package name followed by the interface's simple name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The methods of this interface, in unspecified order.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<ApiMethod>,

    /// Any metadata attached to the interface.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ProtocolBufferOption>,

    /// The version of the api.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Source context for the protocol buffer service represented by this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_context: Option<SourceContext>,

    /// Included interfaces.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixins: Vec<Mixin>,

    /// The source syntax of the service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub syntax: Option<Syntax>,
}

/// ApiMethod represents a method of an API interface.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ApiMethod {
    /// The simple name of this method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// A URL of the input message type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_type_url: Option<String>,

    /// If true, the request is streamed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_streaming: Option<bool>,

    /// The URL of the output message type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_type_url: Option<String>,

    /// If true, the response is streamed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_streaming: Option<bool>,

    /// Any metadata attached to the method.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ProtocolBufferOption>,

    /// The source syntax of this method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub syntax: Option<Syntax>,
}

/// A protocol buffer message type.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Type {
    /// The fully qualified message name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The list of fields.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<Field>,

    /// The list of types appearing in oneof definitions in this type.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oneofs: Vec<String>,

    /// The protocol buffer options.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ProtocolBufferOption>,

    /// The source context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_context: Option<SourceContext>,

    /// The source syntax.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub syntax: Option<Syntax>,
}

/// A single field of a message type.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// The field type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,

    /// The field cardinality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cardinality: Option<Cardinality>,

    /// The field number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<i32>,

    /// The field name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The field type URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_url: Option<String>,

    /// The index of the field type in Type.oneofs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oneof_index: Option<i32>,

    /// Whether to use alternative packed wire representation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packed: Option<bool>,

    /// The protocol buffer options.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ProtocolBufferOption>,

    /// The field JSON name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json_name: Option<String>,

    /// The string value of the default value of this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

/// Basic field types.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    /// Field type unknown.
    TypeUnknown,
    /// Field type double.
    TypeDouble,
    /// Field type float.
    TypeFloat,
    /// Field type int64.
    TypeInt64,
    /// Field type uint64.
    TypeUint64,
    /// Field type int32.
    TypeInt32,
    /// Field type fixed64.
    TypeFixed64,
    /// Field type fixed32.
    TypeFixed32,
    /// Field type bool.
    TypeBool,
    /// Field type string.
    TypeString,
    /// Field type group.
    TypeGroup,
    /// Field type message.
    TypeMessage,
    /// Field type bytes.
    TypeBytes,
    /// Field type uint32.
    TypeUint32,
    /// Field type enum.
    TypeEnum,
    /// Field type sfixed32.
    TypeSfixed32,
    /// Field type sfixed64.
    TypeSfixed64,
    /// Field type sint32.
    TypeSint32,
    /// Field type sint64.
    TypeSint64,
}

/// Whether a field is optional, required, or repeated.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Cardinality {
    /// For fields with unknown cardinality.
    CardinalityUnknown,
    /// For optional fields.
    CardinalityOptional,
    /// For required fields. Proto2 syntax only.
    CardinalityRequired,
    /// For repeated fields.
    CardinalityRepeated,
}

/// Enum type definition.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Enum {
    /// Enum type name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Enum value definitions.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enumvalue: Vec<EnumValue>,

    /// Protocol buffer options.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ProtocolBufferOption>,

    /// The source context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_context: Option<SourceContext>,

    /// The source syntax.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub syntax: Option<Syntax>,
}

/// Enum value definition.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct EnumValue {
    /// Enum value name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Enum value number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<i32>,

    /// Protocol buffer options.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ProtocolBufferOption>,
}

/// A protocol buffer option, which can be attached to a message, field, enumeration, etc.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolBufferOption {
    /// The option's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The option's value packed in an Any message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
}

/// SourceContext represents information about the source of a protobuf element.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SourceContext {
    /// The path-qualified name of the .proto file that contained the associated protobuf element.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

/// Declares an API Interface to be included in this interface.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Mixin {
    /// The fully qualified name of the interface which is included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// If non-empty specifies a path under which inherited HTTP paths are rooted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

/// The syntax in which a protocol buffer element is defined.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Syntax {
    /// Syntax proto2.
    SyntaxProto2,
    /// Syntax proto3.
    SyntaxProto3,
}

/// Documentation provides the information for describing a service.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Documentation {
    /// A short description of what the service does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// The top level pages for the documentation set.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<Page>,

    /// A list of documentation rules that apply to individual API elements.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<DocumentationRule>,

    /// The URL to the root of documentation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_root_url: Option<String>,

    /// Specifies the service root url if the default one is not suitable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_root_url: Option<String>,

    /// An overview of the service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
}

/// Represents a documentation page.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// The name of the page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The Markdown content of the page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Subpages of this page.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subpages: Vec<Page>,
}

/// A documentation rule provides information about individual API elements.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct DocumentationRule {
    /// The selector is a comma-separated list of patterns for any element.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Description of the selected proto element.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Deprecation description of the selected element.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecation_description: Option<String>,
}

/// Endpoint describes a network address of a service that serves a set of APIs.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    /// The canonical name of this endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Unimplemented. A list of IANA ports of the service.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,

    /// The specification of an Internet routable address of API frontend that will handle requests to this API Endpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,

    /// Allowing CORS, aka cross-domain traffic, would allow the backends served from this endpoint to receive and respond to HTTP OPTIONS requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_cors: Option<bool>,
}

/// Quota configuration helps to achieve fairness and budgeting in service usage.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    /// List of QuotaLimit definitions for the service.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub limits: Vec<QuotaLimit>,

    /// List of MetricRule definitions, each one mapping a selected method to one or more metrics.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metric_rules: Vec<MetricRule>,
}

/// QuotaLimit defines a specific limit that applies to a quota.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct QuotaLimit {
    /// Name of the quota limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional. User-visible, extended description for this quota limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Default number of tokens that can be consumed during the specified duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_limit: Option<String>,

    /// Maximum number of tokens that can be consumed during the specified duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_limit: Option<String>,

    /// Free tier value displayed in the Developers Console for this limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free_tier: Option<String>,

    /// Duration of the quota period in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,

    /// The name of the metric this quota limit applies to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<String>,

    /// Specify the unit of the quota limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,

    /// Tiered limit values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<HashMap<String, String>>,

    /// User-visible display name for this limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// Bind API methods to metrics.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MetricRule {
    /// Selects the methods to which this rule applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// Metrics to update when the selected methods are called.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric_costs: Option<HashMap<String, String>>,
}

/// Authentication rules for the service.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Authentication {
    /// A list of authentication rules that apply to individual API methods.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<AuthenticationRule>,

    /// Defines a set of authentication providers that a service supports.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<AuthProvider>,
}

/// Authentication rules for the service.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationRule {
    /// Selects the methods to which this rule applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// The requirements for OAuth credentials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthRequirements>,

    /// If true, the service accepts API keys without any other credential.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_without_credential: Option<bool>,

    /// Requirements for additional authentication providers.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<AuthRequirement>,
}

/// OAuth scopes are a way to define the level of access that is being requested.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct OAuthRequirements {
    /// The list of publicly documented OAuth scopes that are allowed access.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_scopes: Option<String>,
}

/// User-defined authentication requirements, including support for JSON Web Token (JWT).
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AuthRequirement {
    /// id from authentication provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,

    /// NOTE: This will be deprecated soon, once AuthProvider.audiences is implemented and accepted in all the runtime components.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audiences: Option<String>,
}

/// Configuration for an authentication provider, including support for JSON Web Token (JWT).
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AuthProvider {
    /// The unique identifier of the auth provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Identifies the principal that issued the JWT.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,

    /// URL of the provider's public key set to validate signature of the JWT.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwks_uri: Option<String>,

    /// The list of JWT audiences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audiences: Option<String>,

    /// Redirect URL if JWT token is required but not present or is expired.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,

    /// Defines the locations to extract the JWT.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jwt_locations: Vec<JwtLocation>,
}

/// Specifies a location to extract JWT from an API request.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct JwtLocation {
    /// Specifies to extract the JWT from a specific HTTP header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,

    /// Specifies to extract the JWT from a query parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,

    /// The value prefix.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_prefix: Option<String>,
}

/// Configuration controlling usage of a service.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Requirements that must be satisfied before a consumer project can use the service.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<String>,

    /// A list of usage rules that apply to individual API methods.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<UsageRule>,

    /// The full resource name of a channel used for sending notifications to the service producer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub producer_notification_channel: Option<String>,
}

/// Usage configuration for an API method.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UsageRule {
    /// Selects the methods to which this rule applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,

    /// If true, the selected method allows unregistered calls.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_unregistered_calls: Option<bool>,

    /// If true, the selected method should skip service control and the control plane features.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_service_control: Option<bool>,
}
