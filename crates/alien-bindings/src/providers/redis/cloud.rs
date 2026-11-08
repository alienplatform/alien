//! Cloud Redis bindings: resolve the password from where the cloud keeps it.
//!
//! AWS and GCP bindings point at a secret the deployment's controller wrote; Azure's binding
//! points at the database, whose access key is read with `listKeys`. Every load reads again,
//! so a rotated credential is picked up. Each function makes exactly one cloud call and returns
//! its failure to the caller, which owns any retry policy.

#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
use super::resolve_params;
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
use crate::error::{ErrorData, Result};
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
use crate::traits::{RedisConnectionParams, RedisTlsPolicy};
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
use alien_core::bindings::BindingValue;
#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
use alien_error::{AlienError, Context};

#[cfg(any(feature = "aws", feature = "gcp", feature = "azure"))]
fn locator(binding_name: &str, field: &str, value: &BindingValue<String>) -> Result<String> {
    value
        .clone()
        .into_value(binding_name, field)
        .context(ErrorData::config_invalid(
            binding_name,
            format!("Failed to extract '{field}' from Redis binding"),
        ))
}

#[cfg(feature = "aws")]
pub(crate) async fn resolve_elasticache(
    binding_name: &str,
    binding: &alien_core::bindings::ElastiCacheRedisBinding,
    secrets: &dyn alien_aws_clients::secrets_manager::SecretsManagerApi,
) -> Result<RedisConnectionParams> {
    use alien_aws_clients::secrets_manager::GetSecretValueRequest;
    let arn = locator(
        binding_name,
        "passwordSecretArn",
        &binding.password_secret_arn,
    )?;
    let response = secrets
        .get_secret_value(
            GetSecretValueRequest::builder()
                .secret_id(arn.clone())
                .build(),
        )
        .await
        .context(ErrorData::RedisCredentialResolutionFailed {
            binding_name: binding_name.to_string(),
            source_locator: arn.clone(),
            reason: "Secrets Manager GetSecretValue failed".to_string(),
        })?;
    let password = response
        .secret_string
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::RedisCredentialInvalid {
                binding_name: binding_name.to_string(),
                source_locator: arn,
                reason: "SecretString is missing or empty".to_string(),
            })
        })?;
    // ElastiCache certificates chain to the Amazon roots every system trust store carries.
    resolve_params(
        binding_name,
        &binding.host,
        &binding.port,
        &binding.username,
        &password,
        RedisTlsPolicy::system_roots(),
    )
}

#[cfg(feature = "gcp")]
pub(crate) async fn resolve_memorystore(
    binding_name: &str,
    binding: &alien_core::bindings::MemorystoreRedisBinding,
    secrets: &dyn alien_gcp_clients::secret_manager::SecretManagerApi,
) -> Result<RedisConnectionParams> {
    use base64::engine::general_purpose::STANDARD as base64_standard;
    use base64::Engine;
    let name = locator(
        binding_name,
        "passwordSecretName",
        &binding.password_secret_name,
    )?;
    let invalid = |reason: &str| ErrorData::RedisCredentialInvalid {
        binding_name: binding_name.to_string(),
        source_locator: name.clone(),
        reason: reason.to_string(),
    };
    let cas = binding
        .server_ca_certificates
        .clone()
        .into_value(binding_name, "serverCaCertificates")
        .context(ErrorData::config_invalid(
            binding_name,
            "Failed to extract 'serverCaCertificates' from Redis binding",
        ))?;
    // Memorystore signs with a per-instance CA, so the roots come from the binding.
    let tls = RedisTlsPolicy::with_ca_certificates(cas).map_err(|_| {
        AlienError::new(ErrorData::config_invalid(
            binding_name,
            "serverCaCertificates must be a non-empty list of PEM certificates",
        ))
    })?;
    let response = secrets
        .access_secret_version(format!("{name}/versions/latest"))
        .await
        .context(ErrorData::RedisCredentialResolutionFailed {
            binding_name: binding_name.to_string(),
            source_locator: name.clone(),
            reason: "Secret Manager accessSecretVersion failed".to_string(),
        })?;
    let encoded = response
        .payload
        .and_then(|payload| payload.data)
        .ok_or_else(|| AlienError::new(invalid("secret version has no payload")))?;
    let decoded = base64_standard
        .decode(&encoded)
        .map_err(|_| AlienError::new(invalid("payload is not valid base64")))?;
    let password = String::from_utf8(decoded)
        .map_err(|_| AlienError::new(invalid("payload is not valid UTF-8")))?;
    if password.is_empty() {
        return Err(AlienError::new(invalid("secret version payload is empty")));
    }
    resolve_params(
        binding_name,
        &binding.host,
        &binding.port,
        &binding.username,
        &password,
        tls,
    )
}

/// `(resource group, cluster name)` from a database resource ID such as
/// `/subscriptions/s/resourceGroups/rg/providers/Microsoft.Cache/redisEnterprise/c/databases/default`.
#[cfg(feature = "azure")]
pub(crate) fn parse_database_id(id: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = id.trim_matches('/').split('/').collect();
    let at = |key: &str| {
        parts
            .iter()
            .position(|part| part.eq_ignore_ascii_case(key))
            .and_then(|index| parts.get(index + 1))
            .map(|value| value.to_string())
    };
    if at("providers")?.eq_ignore_ascii_case("Microsoft.Cache") && !at("databases")?.is_empty() {
        Some((at("resourceGroups")?, at("redisEnterprise")?))
    } else {
        None
    }
}

#[cfg(feature = "azure")]
pub(crate) async fn resolve_azure_managed_redis(
    binding_name: &str,
    binding: &alien_core::bindings::AzureManagedRedisBinding,
    client: &dyn alien_azure_clients::managed_redis::ManagedRedisApi,
) -> Result<RedisConnectionParams> {
    let database_id = locator(
        binding_name,
        "databaseResourceId",
        &binding.database_resource_id,
    )?;
    let (resource_group, cluster) = parse_database_id(&database_id).ok_or_else(|| {
        AlienError::new(ErrorData::config_invalid(
            binding_name,
            "databaseResourceId must be an Azure Managed Redis database resource ID",
        ))
    })?;
    let keys = client.list_keys(&resource_group, &cluster).await.context(
        ErrorData::RedisCredentialResolutionFailed {
            binding_name: binding_name.to_string(),
            source_locator: database_id.clone(),
            reason: "listKeys failed".to_string(),
        },
    )?;
    let password = keys
        .primary_key
        .filter(|key| !key.is_empty())
        .ok_or_else(|| {
            AlienError::new(ErrorData::RedisCredentialInvalid {
                binding_name: binding_name.to_string(),
                source_locator: database_id,
                reason: "the database has no primary access key (access key authentication may be disabled)".to_string(),
            })
        })?;
    // The endpoint certificate is publicly rooted, so the system trust store verifies it.
    resolve_params(
        binding_name,
        &binding.host,
        &binding.port,
        &binding.username,
        &password,
        RedisTlsPolicy::system_roots(),
    )
}

#[cfg(all(test, feature = "azure"))]
mod azure_tests {
    use super::*;

    #[test]
    fn parses_the_database_id_the_controller_emits() {
        let id = "/subscriptions/s/resourceGroups/my-rg/providers/Microsoft.Cache/redisEnterprise/test-cache/databases/default";
        assert_eq!(
            parse_database_id(id),
            Some(("my-rg".to_string(), "test-cache".to_string()))
        );
        assert_eq!(
            parse_database_id("/subscriptions/s/resourceGroups/rg"),
            None
        );
        assert_eq!(
            parse_database_id(
                "/subscriptions/s/resourceGroups/rg/providers/Microsoft.Sql/servers/x/databases/d"
            ),
            None
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::bindings::BindingValue;

    #[cfg(feature = "aws")]
    mod aws {
        use super::*;
        use alien_aws_clients::secrets_manager::{GetSecretValueResponse, MockSecretsManagerApi};

        const ARN: &str = "arn:aws:secretsmanager:us-east-2:000000000000:secret:cache-auth-AbCdEf";

        fn binding() -> alien_core::bindings::ElastiCacheRedisBinding {
            alien_core::bindings::ElastiCacheRedisBinding {
                host: BindingValue::value("master.cache.use2.cache.amazonaws.com".to_string()),
                port: BindingValue::value(6379),
                username: BindingValue::value("default".to_string()),
                password_secret_arn: BindingValue::value(ARN.to_string()),
            }
        }

        fn response(value: Option<&str>) -> GetSecretValueResponse {
            GetSecretValueResponse {
                arn: Some(ARN.into()),
                name: None,
                version_id: None,
                secret_binary: None,
                secret_string: value.map(str::to_string),
                version_stages: None,
                created_date: None,
            }
        }

        #[tokio::test]
        async fn reads_the_secret_once_and_dials_the_binding_host_over_tls() {
            let mut secrets = MockSecretsManagerApi::new();
            secrets
                .expect_get_secret_value()
                .times(1)
                .withf(|request| request.secret_id == ARN)
                .returning(|_| Ok(response(Some("tok/en+1="))));
            let params = resolve_elasticache("cache", &binding(), &secrets)
                .await
                .unwrap();
            assert_eq!(params.host, "master.cache.use2.cache.amazonaws.com");
            assert_eq!(params.port, 6379);
            assert_eq!(params.username, "default");
            assert_eq!(params.password, "tok/en+1=");
            assert!(params.tls.is_enabled());
        }

        #[tokio::test]
        async fn failed_read_is_retryable_and_names_the_secret_not_its_value() {
            let mut secrets = MockSecretsManagerApi::new();
            secrets.expect_get_secret_value().returning(|_| {
                Err(AlienError::new(
                    alien_client_core::ErrorData::RateLimitExceeded {
                        message: "throttled".to_string(),
                    },
                ))
            });
            let error = resolve_elasticache("cache", &binding(), &secrets)
                .await
                .unwrap_err();
            assert_eq!(error.code, "REDIS_CREDENTIAL_RESOLUTION_FAILED");
            assert!(error.retryable);
            assert!(error.to_string().contains(ARN));
        }

        #[tokio::test]
        async fn empty_secret_is_a_non_retryable_invalid_credential() {
            let mut secrets = MockSecretsManagerApi::new();
            secrets
                .expect_get_secret_value()
                .returning(|_| Ok(response(Some(""))));
            let error = resolve_elasticache("cache", &binding(), &secrets)
                .await
                .unwrap_err();
            assert_eq!(error.code, "REDIS_CREDENTIAL_INVALID");
            assert!(!error.retryable);
        }
    }

    #[cfg(feature = "gcp")]
    mod gcp {
        use super::*;
        use alien_gcp_clients::secret_manager::{
            AccessSecretVersionResponse, MockSecretManagerApi, SecretPayload,
        };
        use base64::engine::general_purpose::STANDARD as base64_standard;
        use base64::Engine;

        const CA: &str = "-----BEGIN CERTIFICATE-----\nroot\n-----END CERTIFICATE-----";

        fn binding(cas: Vec<String>) -> alien_core::bindings::MemorystoreRedisBinding {
            alien_core::bindings::MemorystoreRedisBinding {
                host: BindingValue::value("10.0.0.5".to_string()),
                port: BindingValue::value(6379),
                username: BindingValue::value("default".to_string()),
                server_ca_certificates: BindingValue::value(cas),
                password_secret_name: BindingValue::value("test-cache-auth".to_string()),
            }
        }

        #[tokio::test]
        async fn reads_the_latest_version_and_verifies_against_the_binding_cas() {
            let mut secrets = MockSecretManagerApi::new();
            secrets
                .expect_access_secret_version()
                .times(1)
                .withf(|name| name == "test-cache-auth/versions/latest")
                .returning(|_| {
                    Ok(AccessSecretVersionResponse {
                        name: None,
                        payload: Some(SecretPayload {
                            data: Some(base64_standard.encode("token-value")),
                        }),
                    })
                });
            let params = resolve_memorystore("cache", &binding(vec![CA.to_string()]), &secrets)
                .await
                .unwrap();
            assert_eq!(params.password, "token-value");
            assert_eq!(params.tls.ca_certificates(), [CA.to_string()]);
        }

        #[tokio::test]
        async fn missing_cas_fail_before_any_secret_read() {
            // The mock has no expectation, so a secret read would panic.
            let secrets = MockSecretManagerApi::new();
            let error = resolve_memorystore("cache", &binding(vec![]), &secrets)
                .await
                .unwrap_err();
            assert_eq!(error.code, "BINDING_CONFIG_INVALID");
        }
    }

    #[cfg(feature = "azure")]
    mod azure {
        use super::*;
        use alien_azure_clients::managed_redis::{ManagedRedisAccessKeys, MockManagedRedisApi};

        fn binding() -> alien_core::bindings::AzureManagedRedisBinding {
            alien_core::bindings::AzureManagedRedisBinding {
                host: BindingValue::value("test-cache.eastus.redis.azure.net".to_string()),
                port: BindingValue::value(10000),
                username: BindingValue::value("default".to_string()),
                database_resource_id: BindingValue::value(
                    "/subscriptions/s/resourceGroups/my-rg/providers/Microsoft.Cache/redisEnterprise/test-cache/databases/default".to_string(),
                ),
            }
        }

        #[tokio::test]
        async fn lists_the_database_key_in_the_right_group_and_cluster() {
            let mut client = MockManagedRedisApi::new();
            client
                .expect_list_keys()
                .times(1)
                .withf(|group, cluster| group == "my-rg" && cluster == "test-cache")
                .returning(|_, _| {
                    Ok(serde_json::from_str::<ManagedRedisAccessKeys>(
                        r#"{"primaryKey":"k1","secondaryKey":"k2"}"#,
                    )
                    .unwrap())
                });
            let params = resolve_azure_managed_redis("cache", &binding(), &client)
                .await
                .unwrap();
            assert_eq!(params.port, 10000);
            assert_eq!(params.password, "k1");
            assert!(params.tls.is_enabled());
        }

        #[tokio::test]
        async fn disabled_key_authentication_is_an_invalid_credential() {
            let mut client = MockManagedRedisApi::new();
            client.expect_list_keys().returning(|_, _| {
                Ok(serde_json::from_str::<ManagedRedisAccessKeys>("{}").unwrap())
            });
            let error = resolve_azure_managed_redis("cache", &binding(), &client)
                .await
                .unwrap_err();
            assert_eq!(error.code, "REDIS_CREDENTIAL_INVALID");
        }
    }
}
