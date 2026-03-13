//! Type-safe binding parameter definitions
//!
//! This module defines structs that represent the runtime parameters needed by bindings
//! to interact with cloud resources. These structs are used by:
//! - Controllers when returning binding parameters directly  
//! - Template generators when creating CloudFormation/Terraform templates (using Fn::ToJsonString)
//! - Bindings when consuming runtime parameters (parsing JSON)
//!
//! This provides type safety and ensures consistency across all parts of the system.

use crate::error::ErrorData;
use alien_error::{AlienError, Context, IntoAlienError};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

mod artifact_registry;
mod build;
mod container;
mod function;
mod kv;
mod queue;
mod service_account;
mod storage;
mod vault;

pub use artifact_registry::{
    AcrArtifactRegistryBinding, ArtifactRegistryBinding, EcrArtifactRegistryBinding,
    GarArtifactRegistryBinding, LocalArtifactRegistryBinding,
};
pub use build::{
    AcaBuildBinding, BuildBinding, CloudbuildBuildBinding, CodebuildBuildBinding, LocalBuildBinding,
};
pub use container::{
    ContainerBinding, HorizonContainerBinding, KubernetesContainerBinding, LocalContainerBinding,
};
pub use function::{
    CloudRunFunctionBinding, ContainerAppFunctionBinding, FunctionBinding,
    KubernetesFunctionBinding, LambdaFunctionBinding, LocalFunctionBinding,
};
pub use kv::{
    DynamodbKvBinding, FirestoreKvBinding, KvBinding, LocalKvBinding, RedisKvBinding,
    TableStorageKvBinding,
};
pub use queue::{PubSubQueueBinding, QueueBinding, ServiceBusQueueBinding, SqsQueueBinding};
pub use service_account::{
    AwsServiceAccountBinding, AzureServiceAccountBinding, GcpServiceAccountBinding,
    ServiceAccountBinding,
};
pub use storage::{
    BlobStorageBinding, GcsStorageBinding, LocalStorageBinding, S3StorageBinding, StorageBinding,
};
pub use vault::{
    KeyVaultBinding, KubernetesSecretVaultBinding, LocalVaultBinding, ParameterStoreVaultBinding,
    SecretManagerVaultBinding, VaultBinding,
};

/// Represents a value that can be either a concrete value, a template expression,
/// or a reference to a Kubernetes Secret
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum BindingValue<T> {
    /// A concrete value (used by controllers)
    Value(T),
    /// A Kubernetes Secret reference (must come before Expression)
    #[serde(rename_all = "camelCase")]
    SecretRef { secret_ref: SecretReference },
    /// A template expression (used by IaC template generators)
    #[cfg_attr(feature = "jsonschema", schemars(skip))]
    Expression(JsonValue),
}

/// Reference to a Kubernetes Secret
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct SecretReference {
    pub name: String,
    pub key: String,
}

impl<T> BindingValue<T> {
    /// Creates a concrete value
    pub fn value(val: T) -> Self {
        Self::Value(val)
    }

    /// Creates a template expression
    pub fn expression(expr: JsonValue) -> Self {
        Self::Expression(expr)
    }

    /// Extracts the concrete value, returning an error if this is a template expression or SecretRef
    pub fn into_value(self, binding_name: &str, field_name: &str) -> crate::error::Result<T> {
        match self {
            BindingValue::Value(val) => Ok(val),
            BindingValue::Expression(_) => Err(AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: format!("Template expressions not supported in runtime bindings for field '{}'", field_name),
            })),
            BindingValue::SecretRef { .. } => Err(AlienError::new(ErrorData::BindingConfigInvalid {
                binding_name: binding_name.to_string(),
                reason: format!("SecretRef not resolved for field '{}' - this should have been resolved by the controller", field_name),
            }))
        }
    }
}

impl<T> From<T> for BindingValue<T> {
    fn from(val: T) -> Self {
        Self::Value(val)
    }
}

impl From<&str> for BindingValue<String> {
    fn from(val: &str) -> Self {
        Self::Value(val.to_string())
    }
}

impl From<JsonValue> for BindingValue<String> {
    fn from(val: JsonValue) -> Self {
        Self::Expression(val)
    }
}

/// Helper function to serialize binding struct as JSON for environment variables
pub fn serialize_binding_as_env_var<T: Serialize>(
    binding_name: &str,
    binding: &T,
) -> crate::error::Result<HashMap<String, String>> {
    let mut env_vars = HashMap::new();
    let key = binding_env_var_name(binding_name);
    let binding_json = serde_json::to_string(binding).into_alien_error().context(
        ErrorData::BindingConfigInvalid {
            binding_name: binding_name.to_string(),
            reason: "Failed to serialize binding to JSON".to_string(),
        },
    )?;
    env_vars.insert(key, binding_json);
    Ok(env_vars)
}

/// Helper function to serialize binding struct for CloudFormation templates
pub fn serialize_binding_for_template<T: Serialize>(
    binding_name: &str,
    binding: &T,
) -> crate::error::Result<HashMap<String, JsonValue>> {
    let mut env_vars = HashMap::new();
    let key = binding_env_var_name(binding_name);
    let binding_json = serde_json::to_value(binding).into_alien_error().context(
        ErrorData::BindingConfigInvalid {
            binding_name: binding_name.to_string(),
            reason: "Failed to serialize binding to JSON for template".to_string(),
        },
    )?;

    // Wrap in Fn::ToJsonString for CloudFormation
    env_vars.insert(
        key,
        JsonValue::Object({
            let mut map = serde_json::Map::new();
            map.insert("Fn::ToJsonString".to_string(), binding_json);
            map
        }),
    );

    Ok(env_vars)
}

/// Helper function to generate the environment variable name for a binding
pub fn binding_env_var_name(binding_name: &str) -> String {
    format!(
        "ALIEN_{}_BINDING",
        binding_name.replace('-', "_").to_uppercase()
    )
}

/// Helper function to parse binding from environment variable
pub fn parse_binding_from_env<T: for<'de> Deserialize<'de>>(
    env: &HashMap<String, String>,
    binding_name: &str,
) -> crate::error::Result<T> {
    let key = binding_env_var_name(binding_name);
    let json_str = env.get(&key).ok_or_else(|| {
        AlienError::new(ErrorData::BindingEnvVarMissing {
            binding_name: binding_name.to_string(),
            env_var: key.clone(),
        })
    })?;

    serde_json::from_str(json_str)
        .into_alien_error()
        .context(ErrorData::BindingJsonParseFailed {
            binding_name: binding_name.to_string(),
            reason: "Invalid JSON format".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{ArtifactRegistryBinding, BuildBinding, StorageBinding};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn test_serialize_storage_binding_as_env_var() {
        let binding = StorageBinding::s3("my-bucket");

        let env_vars = serialize_binding_as_env_var("TEST", &binding).unwrap();

        assert_eq!(env_vars.len(), 1);
        let json_str = env_vars.get("ALIEN_TEST_BINDING").unwrap();
        let parsed: StorageBinding = serde_json::from_str(json_str).unwrap();
        assert_eq!(binding, parsed);
    }

    #[test]
    fn test_serialize_binding_for_template() {
        let binding = StorageBinding::S3(S3StorageBinding {
            bucket_name: BindingValue::expression(json!({"Ref": "MyBucket"})),
        });

        let env_vars = serialize_binding_for_template("TEST", &binding).unwrap();

        assert_eq!(env_vars.len(), 1);
        let fn_to_json_string = env_vars.get("ALIEN_TEST_BINDING").unwrap();

        // Should be wrapped in Fn::ToJsonString
        assert!(fn_to_json_string.get("Fn::ToJsonString").is_some());
    }

    #[test]
    fn test_artifact_registry_binding_roundtrip() {
        let binding = ArtifactRegistryBinding::ecr(
            "my-project",
            Some("arn:aws:iam::123456789012:role/PullRole".to_string()),
            None::<String>,
        );

        let env_vars = serialize_binding_as_env_var("TEST", &binding).unwrap();

        let reconstructed: ArtifactRegistryBinding =
            parse_binding_from_env(&env_vars, "TEST").unwrap();
        assert_eq!(binding, reconstructed);
    }

    #[test]
    fn test_service_type_serialization() {
        // Test S3 storage
        let s3_binding = StorageBinding::s3("my-bucket");
        let s3_json = serde_json::to_string(&s3_binding).unwrap();
        assert!(s3_json.contains(r#""service":"s3""#));

        // Test ECR registry
        let ecr_binding = ArtifactRegistryBinding::ecr("my-repo", None::<String>, None::<String>);
        let ecr_json = serde_json::to_string(&ecr_binding).unwrap();
        assert!(ecr_json.contains(r#""service":"ecr""#));

        // Test CodeBuild
        let build_binding = BuildBinding::codebuild("my-project", HashMap::new(), None);
        let build_json = serde_json::to_string(&build_binding).unwrap();
        assert!(build_json.contains(r#""service":"codebuild""#));
    }

    #[test]
    fn test_cross_provider_bindings() {
        // Test that we can mix different service types in one environment
        let storage_binding = StorageBinding::s3("prod-bucket");
        let registry_binding = ArtifactRegistryBinding::acr("myregistry", "mygroup");
        let build_binding = BuildBinding::cloudbuild(
            HashMap::new(),
            "build@project.iam.gserviceaccount.com",
            None,
        );

        // Serialize all bindings
        let storage_env = serialize_binding_as_env_var("STORAGE", &storage_binding).unwrap();
        let registry_env = serialize_binding_as_env_var("REGISTRY", &registry_binding).unwrap();
        let build_env = serialize_binding_as_env_var("BUILD", &build_binding).unwrap();

        // All should work together
        assert!(storage_env.contains_key("ALIEN_STORAGE_BINDING"));
        assert!(registry_env.contains_key("ALIEN_REGISTRY_BINDING"));
        assert!(build_env.contains_key("ALIEN_BUILD_BINDING"));
    }

    #[test]
    fn test_binding_value_secret_ref() {
        // Test SecretRef serialization
        let secret_ref: BindingValue<String> = BindingValue::SecretRef {
            secret_ref: SecretReference {
                name: "my-secret".to_string(),
                key: "password".to_string(),
            },
        };

        let json = serde_json::to_string(&secret_ref).unwrap();
        assert!(json.contains(r#""secretRef""#));
        assert!(json.contains(r#""name":"my-secret""#));
        assert!(json.contains(r#""key":"password""#));

        // Test deserialization
        let parsed: BindingValue<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(secret_ref, parsed);
    }

    #[test]
    fn test_binding_value_into_value_secret_ref() {
        let secret_ref: BindingValue<String> = BindingValue::SecretRef {
            secret_ref: SecretReference {
                name: "my-secret".to_string(),
                key: "password".to_string(),
            },
        };

        let result = secret_ref.into_value("test", "password");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("SecretRef not resolved"));
    }

    #[test]
    fn test_binding_value_variants() {
        // Test Value variant
        let value: BindingValue<String> = BindingValue::value("test".to_string());
        assert_eq!(value.into_value("test", "field").unwrap(), "test");

        // Test Expression variant
        let expr: BindingValue<String> = BindingValue::expression(json!({"Ref": "Test"}));
        assert!(expr.into_value("test", "field").is_err());
    }
}
