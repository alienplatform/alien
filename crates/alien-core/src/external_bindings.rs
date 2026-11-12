//! External bindings for pre-existing infrastructure services.
//!
//! External bindings allow using existing infrastructure (MinIO, Kafka, Redis, etc.)
//! instead of having Alien provision cloud resources. This is required for Kubernetes
//! platform deployments and optional for cloud platforms (to override specific resources).

use std::collections::HashMap;

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::bindings::{
    ArtifactRegistryBinding, BindingValue, ContainerAppsEnvironmentBinding, ExternalAiBinding,
    KvBinding, PostgresBinding, QueueBinding, RedisBinding, StorageBinding, VaultBinding,
};
use crate::error::ErrorData;
use crate::resource::{ResourceOutputs, ResourceType};
use crate::resources::AzureContainerAppsEnvironmentOutputs;
use crate::{CustomResourceOutputs, Resource, ResourceTypeDescriptor};

/// Represents a binding to pre-existing infrastructure.
///
/// The binding type must match the resource type it's applied to.
/// Validated at runtime by the executor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", remote = "Self")]
pub enum ExternalBinding {
    /// External storage binding (S3-compatible, GCS, Blob Storage)
    Storage(StorageBinding),
    /// External queue binding (Kafka, SQS, etc.)
    Queue(QueueBinding),
    /// External KV binding (Redis, etc.)
    Kv(KvBinding),
    /// External artifact registry binding (OCI registry)
    ArtifactRegistry(ArtifactRegistryBinding),
    /// External vault binding (HashiCorp Vault, etc.)
    Vault(VaultBinding),
    /// External Azure Container Apps Environment binding (pre-existing environment)
    ContainerAppsEnvironment(ContainerAppsEnvironmentBinding),
    /// External Postgres binding (operator-provided / BYO database)
    Postgres(PostgresBinding),
    /// External Redis binding (operator-provided Redis-compatible server)
    Redis(RedisBinding),
    /// External AI provider binding (BYO-key OpenAI/Anthropic, etc.)
    Ai(ExternalAiBinding),
    /// A binding to an existing resource of a type an extension defines. It serializes to the
    /// same object as the variants above, with the extension's type as the tag.
    #[serde(skip)]
    Custom(CustomExternalBinding),
}

// `remote = "Self"` turns the derived impls into inherent fns, which these two call for the
// built-in variants.
impl Serialize for ExternalBinding {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ExternalBinding::Custom(custom) => custom.serialize(serializer),
            built_in => ExternalBinding::serialize(built_in, serializer),
        }
    }
}

/// Only a tag that names an extension's type reads as `Custom`. A built-in tag takes its own
/// variant's shape and any other tag is refused, so a malformed built-in binding never loads as
/// custom.
impl<'de> Deserialize<'de> for ExternalBinding {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let is_custom = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(crate::custom_resource::is_custom_resource_type);
        if is_custom {
            serde_json::from_value::<CustomExternalBinding>(value)
                .map(ExternalBinding::Custom)
                .map_err(serde::de::Error::custom)
        } else {
            ExternalBinding::deserialize(value).map_err(serde::de::Error::custom)
        }
    }
}

// Utoipa drops a variant marked `#[serde(skip)]`, as `Custom` is, so `ExternalBinding`'s schema
// is this enum's, with `Custom` added. Its docs repeat `ExternalBinding`'s word for word: they
// become the TypeScript SDK's descriptions.
/// Represents a binding to pre-existing infrastructure.
///
/// The binding type must match the resource type it's applied to.
/// Validated at runtime by the executor.
#[cfg(feature = "openapi")]
#[derive(Serialize, utoipa::ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
enum BuiltInExternalBinding {
    /// External storage binding (S3-compatible, GCS, Blob Storage)
    Storage(StorageBinding),
    /// External queue binding (Kafka, SQS, etc.)
    Queue(QueueBinding),
    /// External KV binding (Redis, etc.)
    Kv(KvBinding),
    /// External artifact registry binding (OCI registry)
    ArtifactRegistry(ArtifactRegistryBinding),
    /// External vault binding (HashiCorp Vault, etc.)
    Vault(VaultBinding),
    /// External Azure Container Apps Environment binding (pre-existing environment)
    ContainerAppsEnvironment(ContainerAppsEnvironmentBinding),
    /// External Postgres binding (operator-provided / BYO database)
    Postgres(PostgresBinding),
    /// External Redis binding (operator-provided Redis-compatible server)
    Redis(RedisBinding),
    /// External AI provider binding (BYO-key OpenAI/Anthropic, etc.)
    Ai(ExternalAiBinding),
}

#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for ExternalBinding {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        use utoipa::openapi::schema::Schema;
        use utoipa::openapi::{Ref, RefOr};
        use utoipa::ToSchema as _;
        let mut schema = BuiltInExternalBinding::schema();
        if let RefOr::T(Schema::OneOf(one_of)) = &mut schema {
            one_of
                .items
                .push(Ref::from_schema_name(CustomExternalBinding::name()).into());
        }
        schema
    }
}

#[cfg(feature = "openapi")]
impl utoipa::ToSchema for ExternalBinding {
    fn schemas(
        schemas: &mut Vec<(
            String,
            utoipa::openapi::RefOr<utoipa::openapi::schema::Schema>,
        )>,
    ) {
        use utoipa::PartialSchema as _;
        BuiltInExternalBinding::schemas(schemas);
        schemas.push((
            CustomExternalBinding::name().into_owned(),
            CustomExternalBinding::schema(),
        ));
        CustomExternalBinding::schemas(schemas);
    }
}

/// A binding to an existing resource of a type an extension defines, such as a DNS zone the
/// customer already has. Its fields are the resource's outputs, as the type's descriptor declares
/// them in `schemas.outputs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CustomExternalBinding {
    /// The extension's type, such as `acme/dns-zone`.
    #[serde(rename = "type")]
    #[cfg_attr(
        feature = "openapi",
        schema(value_type = crate::heartbeat::CustomResourceType)
    )]
    pub resource_type: ResourceType,
    /// The service the existing resource lives in, such as `route53`.
    pub service: String,
    /// The resource's outputs, such as its `zoneId`.
    #[serde(flatten)]
    pub fields: serde_json::Map<String, serde_json::Value>,
}

impl CustomExternalBinding {
    /// Checks the fields against the outputs schema in `descriptor`, failing closed: a schema
    /// keyword the check doesn't read refuses the binding.
    pub fn validate(&self, descriptor: &ResourceTypeDescriptor) -> Result<(), String> {
        let problems = crate::binding_schema::problems(
            &descriptor.schemas.outputs,
            &serde_json::Value::Object(self.fields.clone()),
        );
        if problems.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "The binding for type '{}' does not match the outputs its extension declares: {}",
                self.resource_type,
                problems.join("; ")
            ))
        }
    }
}

/// The binding's own fields beside the outputs, which an outputs schema must not name: they would
/// read as the binding's tag and service instead of outputs.
pub const CUSTOM_BINDING_RESERVED_FIELDS: [&str; 2] = ["type", "service"];

/// Map from resource ID to external binding.
///
/// Validated at runtime: binding type must match resource type.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(transparent)]
pub struct ExternalBindings(pub HashMap<String, ExternalBinding>);

impl ExternalBindings {
    /// Creates an empty ExternalBindings map.
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Returns true if there are no external bindings.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Checks if a binding exists for the given resource ID.
    pub fn has(&self, resource_id: &str) -> bool {
        self.0.contains_key(resource_id)
    }

    /// Gets an external binding by resource ID.
    pub fn get(&self, resource_id: &str) -> Option<&ExternalBinding> {
        self.0.get(resource_id)
    }

    /// Gets a storage binding for the given resource ID.
    /// Returns an error if the binding exists but is not a Storage type.
    pub fn get_storage(&self, id: &str) -> crate::error::Result<Option<&StorageBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Storage(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "storage".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets a queue binding for the given resource ID.
    /// Returns an error if the binding exists but is not a Queue type.
    pub fn get_queue(&self, id: &str) -> crate::error::Result<Option<&QueueBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Queue(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "queue".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets a KV binding for the given resource ID.
    /// Returns an error if the binding exists but is not a Kv type.
    pub fn get_kv(&self, id: &str) -> crate::error::Result<Option<&KvBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Kv(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "kv".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets an artifact registry binding for the given resource ID.
    /// Returns an error if the binding exists but is not an ArtifactRegistry type.
    pub fn get_artifact_registry(
        &self,
        id: &str,
    ) -> crate::error::Result<Option<&ArtifactRegistryBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::ArtifactRegistry(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "artifact_registry".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets a vault binding for the given resource ID.
    /// Returns an error if the binding exists but is not a Vault type.
    pub fn get_vault(&self, id: &str) -> crate::error::Result<Option<&VaultBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Vault(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "vault".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets an AI binding for the given resource ID.
    /// Returns an error if the binding exists but is not an Ai type.
    pub fn get_ai(&self, id: &str) -> crate::error::Result<Option<&ExternalAiBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Ai(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "ai".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Gets a container apps environment binding for the given resource ID.
    /// Returns an error if the binding exists but is not a ContainerAppsEnvironment type.
    pub fn get_container_apps_environment(
        &self,
        id: &str,
    ) -> crate::error::Result<Option<&ContainerAppsEnvironmentBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::ContainerAppsEnvironment(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "azure_container_apps_environment".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Returns an error if the binding exists but is not a Postgres type.
    pub fn get_postgres(&self, id: &str) -> crate::error::Result<Option<&PostgresBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Postgres(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "postgres".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Returns an error if the binding exists but is not a Redis type.
    pub fn get_redis(&self, id: &str) -> crate::error::Result<Option<&RedisBinding>> {
        match self.0.get(id) {
            Some(ExternalBinding::Redis(b)) => Ok(Some(b)),
            Some(other) => Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
                resource_id: id.to_string(),
                expected: "redis".to_string(),
                actual: other.binding_type().to_string(),
            })),
            None => Ok(None),
        }
    }

    /// Inserts an external binding for a resource.
    pub fn insert(&mut self, resource_id: impl Into<String>, binding: ExternalBinding) {
        self.0.insert(resource_id.into(), binding);
    }
}

impl ExternalBinding {
    /// Returns the type name of this binding variant.
    pub fn binding_type(&self) -> &str {
        match self {
            ExternalBinding::Storage(_) => "storage",
            ExternalBinding::Queue(_) => "queue",
            ExternalBinding::Kv(_) => "kv",
            ExternalBinding::ArtifactRegistry(_) => "artifact_registry",
            ExternalBinding::Vault(_) => "vault",
            ExternalBinding::ContainerAppsEnvironment(_) => "azure_container_apps_environment",
            ExternalBinding::Postgres(_) => "postgres",
            ExternalBinding::Redis(_) => "redis",
            ExternalBinding::Ai(_) => "ai",
            ExternalBinding::Custom(custom) => custom.resource_type.as_ref(),
        }
    }

    /// Converts this external binding into resource outputs that dependent resources
    /// can read via `get_resource_outputs()`.
    ///
    /// Infrastructure bindings (Container Apps Environment) produce typed outputs so that
    /// dependent resources like functions and builds can read the environment's name,
    /// resource ID, and resource group. Application-level bindings (Storage, Queue, KV, etc.)
    /// return `None` — they are consumed via `remote_binding_params` and environment variables
    /// rather than `get_resource_outputs()`.
    pub fn to_resource_outputs(&self) -> Option<ResourceOutputs> {
        match self {
            ExternalBinding::ContainerAppsEnvironment(binding) => {
                // Extract concrete values from BindingValue wrappers.
                // External bindings for pre-provisioned resources always use concrete values.
                let environment_name = match &binding.environment_name {
                    BindingValue::Value(v) => v.clone(),
                    _ => return None,
                };
                let resource_id = match &binding.resource_id {
                    BindingValue::Value(v) => v.clone(),
                    _ => return None,
                };
                let resource_group_name = match &binding.resource_group_name {
                    BindingValue::Value(v) => v.clone(),
                    _ => return None,
                };
                let default_domain = match &binding.default_domain {
                    BindingValue::Value(v) => v.clone(),
                    _ => return None,
                };
                let static_ip = binding.static_ip.as_ref().and_then(|v| match v {
                    BindingValue::Value(v) => Some(v.clone()),
                    _ => None,
                });

                Some(ResourceOutputs::new(AzureContainerAppsEnvironmentOutputs {
                    environment_name,
                    resource_id,
                    resource_group_name,
                    default_domain,
                    static_ip,
                    custom_domain_verification_id: None,
                }))
            }
            // A custom binding's fields are the resource's outputs.
            ExternalBinding::Custom(binding) => Some(ResourceOutputs::new(
                CustomResourceOutputs::new(binding.resource_type.clone(), binding.fields.clone()),
            )),
            // Application-level bindings are consumed via remote_binding_params, not outputs
            _ => None,
        }
    }

    /// Serializes this external binding to the JSON value injected into the worker
    /// environment as `ALIEN_<NAME>_BINDING`, matching the shape the SDK parses.
    ///
    /// The AI arm wraps the carried `ExternalAiBinding` in `AiBinding::External` so
    /// the value keeps the `service` tag the SDK's `ai(name)` parser discriminates
    /// on; a bare `ExternalAiBinding` would lack it. Every other arm serializes the
    /// carried binding directly, like the runtime controllers' `get_binding_params`.
    pub fn to_env_binding_value(&self) -> serde_json::Result<serde_json::Value> {
        match self {
            ExternalBinding::Storage(b) => serde_json::to_value(b),
            ExternalBinding::Queue(b) => serde_json::to_value(b),
            ExternalBinding::Kv(b) => serde_json::to_value(b),
            ExternalBinding::ArtifactRegistry(b) => serde_json::to_value(b),
            ExternalBinding::Vault(b) => serde_json::to_value(b),
            ExternalBinding::ContainerAppsEnvironment(b) => serde_json::to_value(b),
            ExternalBinding::Postgres(b) => serde_json::to_value(b),
            ExternalBinding::Redis(b) => serde_json::to_value(b),
            ExternalBinding::Ai(b) => {
                serde_json::to_value(crate::bindings::AiBinding::External(b.clone()))
            }
            // As a built-in binding, the value carries the service and the fields, not the tag.
            ExternalBinding::Custom(b) => {
                let mut value = serde_json::to_value(b)?;
                if let serde_json::Value::Object(fields) = &mut value {
                    fields.remove("type");
                }
                Ok(value)
            }
        }
    }
}

/// Validates that an external binding type matches the resource type.
pub fn validate_binding_type(
    resource: &Resource,
    binding: &ExternalBinding,
) -> crate::error::Result<()> {
    let resource_type = resource.resource_type();
    let resource_type_str = resource_type.as_ref();

    let valid = match (resource_type_str, binding) {
        ("storage", ExternalBinding::Storage(_)) => true,
        ("queue", ExternalBinding::Queue(_)) => true,
        ("kv", ExternalBinding::Kv(_)) => true,
        ("artifact_registry", ExternalBinding::ArtifactRegistry(_)) => true,
        ("vault", ExternalBinding::Vault(_)) => true,
        ("azure_container_apps_environment", ExternalBinding::ContainerAppsEnvironment(_)) => true,
        ("postgres", ExternalBinding::Postgres(_)) => true,
        ("redis", ExternalBinding::Redis(_)) => true,
        ("ai", ExternalBinding::Ai(_)) => true,
        (_, ExternalBinding::Custom(custom)) => custom.resource_type == resource_type,
        _ => false,
    };

    if !valid {
        return Err(AlienError::new(ErrorData::ExternalBindingTypeMismatch {
            resource_id: resource.id().to_string(),
            expected: resource_type_str.to_string(),
            actual: binding.binding_type().to_string(),
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{KvBinding, StorageBinding};

    #[test]
    fn test_external_bindings_storage() {
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "data-storage",
            ExternalBinding::Storage(StorageBinding::s3("my-bucket")),
        );

        assert!(bindings.has("data-storage"));
        assert!(bindings.get_storage("data-storage").unwrap().is_some());
        assert!(bindings.get_queue("data-storage").is_err()); // Wrong type
    }

    #[test]
    fn test_external_bindings_kv() {
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "cache",
            ExternalBinding::Kv(KvBinding::redis("redis://localhost:6379")),
        );

        assert!(bindings.has("cache"));
        assert!(bindings.get_kv("cache").unwrap().is_some());
        assert!(bindings.get_storage("cache").is_err()); // Wrong type
    }

    #[test]
    fn test_external_bindings_ai() {
        use crate::bindings::ExternalAiBinding;

        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "llm",
            ExternalBinding::Ai(ExternalAiBinding {
                provider: "openai".to_string(),
                api_key: "sk-test".into(),
            }),
        );

        assert!(bindings.has("llm"));
        assert_eq!(bindings.get("llm").unwrap().binding_type(), "ai");
        assert_eq!(bindings.get_ai("llm").unwrap().unwrap().provider, "openai");
        assert!(bindings.get_kv("llm").is_err()); // Wrong type
    }

    fn zone_binding(fields: serde_json::Value) -> ExternalBinding {
        let mut value = serde_json::json!({ "type": "acme/dns-zone", "service": "route53" });
        value
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        serde_json::from_value(value).unwrap()
    }

    fn zone_descriptor() -> ResourceTypeDescriptor {
        serde_json::from_value(serde_json::json!({
            "lifecycles": ["frozen", "live"],
            "holdsData": false,
            "setup": { "cloudformation": [], "terraform": [], "helm": ["kubernetes"], "importers": [] },
            "controllerPlatforms": [],
            "permissionSets": [],
            "link": { "permissionSets": [] },
            "schemas": {
                "config": {},
                "outputs": {
                    "type": "object",
                    "required": ["zoneId"],
                    "properties": { "zoneId": { "type": "string" }, "ttl": { "type": ["integer", "null"] } }
                }
            }
        }))
        .unwrap()
    }

    /// A custom binding reads back as it was written, and becomes the resource's outputs and the
    /// value a linked workload gets.
    #[test]
    fn a_custom_binding_is_the_resource_s_outputs() {
        let json =
            serde_json::json!({ "type": "acme/dns-zone", "service": "route53", "zoneId": "Z1" });
        let binding: ExternalBinding = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&binding).unwrap(), json);
        assert_eq!(binding.binding_type(), "acme/dns-zone");
        assert_eq!(
            serde_json::to_value(binding.to_resource_outputs().unwrap()).unwrap(),
            serde_json::json!({ "type": "acme/dns-zone", "zoneId": "Z1" })
        );
        assert_eq!(
            binding.to_env_binding_value().unwrap(),
            serde_json::json!({ "service": "route53", "zoneId": "Z1" })
        );
    }

    /// A malformed built-in binding, a reserved prefix and an unknown plain tag stay errors.
    #[test]
    fn only_an_extension_s_type_reads_as_a_custom_binding() {
        for json in [
            r#"{"type":"storage","service":"bogus","bucketName":"b"}"#,
            r#"{"type":"alien/zone","service":"route53","zoneId":"Z1"}"#,
            r#"{"type":"storge","service":"s3","bucketName":"b"}"#,
        ] {
            assert!(
                serde_json::from_str::<ExternalBinding>(json).is_err(),
                "{json} must not read"
            );
        }
    }

    #[test]
    fn a_custom_binding_matches_only_its_own_type() {
        let zone = Resource::new(
            crate::CustomResource::from_untagged(
                "acme/dns-zone",
                serde_json::json!({ "id": "zone", "domain": "a.example" }),
            )
            .unwrap(),
        );
        let other = Resource::new(
            crate::CustomResource::from_untagged(
                "acme/record",
                serde_json::json!({ "id": "zone" }),
            )
            .unwrap(),
        );
        let binding = zone_binding(serde_json::json!({ "zoneId": "Z1" }));
        assert!(validate_binding_type(&zone, &binding).is_ok());
        assert!(validate_binding_type(&other, &binding).is_err());
        let storage = Resource::new(crate::Storage::new("zone".to_string()).build());
        assert!(validate_binding_type(&storage, &binding).is_err());
    }

    #[test]
    fn a_custom_binding_is_checked_against_the_type_s_outputs() {
        let descriptor = zone_descriptor();
        assert!(zone_binding(serde_json::json!({ "zoneId": "Z1" }))
            .as_custom()
            .validate(&descriptor)
            .is_ok());
        assert!(
            zone_binding(serde_json::json!({ "zoneId": "Z1", "ttl": 60 }))
                .as_custom()
                .validate(&descriptor)
                .is_ok()
        );
        for (fields, problem) in [
            (serde_json::json!({}), "the binding has no 'zoneId'"),
            (
                serde_json::json!({ "zoneID": "Z1", "zoneId": "Z1" }),
                "'zoneID' is not one of the outputs its schema names",
            ),
            (
                serde_json::json!({ "zoneId": 7 }),
                "'zoneId' is number, where the outputs schema says string",
            ),
            (
                serde_json::json!({ "zoneId": "Z1", "ttl": "60" }),
                "'ttl' is string, where the outputs schema says integer or null",
            ),
        ] {
            let error = zone_binding(fields.clone())
                .as_custom()
                .validate(&descriptor)
                .unwrap_err();
            assert!(error.contains(problem), "{fields}: {error}");
        }
    }

    impl ExternalBinding {
        fn as_custom(&self) -> &CustomExternalBinding {
            match self {
                ExternalBinding::Custom(custom) => custom,
                other => panic!("{other:?} is not custom"),
            }
        }
    }

    #[test]
    fn test_external_bindings_serialization() {
        let mut bindings = ExternalBindings::new();
        bindings.insert(
            "data",
            ExternalBinding::Storage(StorageBinding::s3("test-bucket")),
        );

        let json = serde_json::to_string(&bindings).unwrap();
        let deserialized: ExternalBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(bindings, deserialized);
    }
}
