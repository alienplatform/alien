//! Cloud Redis bindings: resolve the password from where the cloud keeps it.
//!
//! The AWS binding points at a secret the deployment's controller wrote. Azure's binding points
//! at the database, whose access key is read with `listKeys`, and GCP's at the instance, whose
//! token is read from Memorystore. Every load reads again,
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

/// `(project, location, instance id)` from `projects/<p>/locations/<l>/instances/<i>`.
#[cfg(feature = "gcp")]
pub(crate) fn parse_instance_name(name: &str) -> Option<(String, String, String)> {
    let parts: Vec<&str> = name.split('/').collect();
    match parts.as_slice() {
        ["projects", project, "locations", location, "instances", instance]
            if !project.is_empty() && !location.is_empty() && !instance.is_empty() =>
        {
            Some((
                project.to_string(),
                location.to_string(),
                instance.to_string(),
            ))
        }
        _ => None,
    }
}

/// The newest ACTIVE token. During a rotation both tokens are valid, and the newer one survives
/// the delete of the old one.
#[cfg(feature = "gcp")]
fn newest_active_token(tokens: Vec<alien_gcp_clients::memorystore::AuthToken>) -> Option<String> {
    tokens
        .into_iter()
        .filter(|token| token.state.as_deref() == Some("ACTIVE") && !token.token.is_empty())
        .max_by_key(|token| {
            // Google emits a varying number of fractional digits, so strings do not sort.
            token
                .create_time
                .as_deref()
                .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
        })
        .map(|token| token.token)
}

#[cfg(feature = "gcp")]
pub(crate) async fn resolve_memorystore(
    binding_name: &str,
    binding: &alien_core::bindings::MemorystoreRedisBinding,
    memorystore: &dyn alien_gcp_clients::memorystore::MemorystoreApi,
    client_project_id: &str,
) -> Result<RedisConnectionParams> {
    let name = locator(binding_name, "instanceName", &binding.instance_name)?;
    let (project, location, instance) = parse_instance_name(&name).ok_or_else(|| {
        AlienError::new(ErrorData::config_invalid(
            binding_name,
            "instanceName must be projects/<project>/locations/<region>/instances/<id>",
        ))
    })?;
    // The client reads from its own project, so a binding for another project would read the
    // wrong instance's token (or fail confusingly).
    if project != client_project_id {
        return Err(AlienError::new(ErrorData::config_invalid(
            binding_name,
            format!(
                "instanceName is in project '{project}' but the workload runs in '{client_project_id}'"
            ),
        )));
    }
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
    let tokens = memorystore
        .list_auth_tokens(&location, &instance, "default")
        .await
        .context(ErrorData::RedisCredentialResolutionFailed {
            binding_name: binding_name.to_string(),
            source_locator: name.clone(),
            reason: "Memorystore authTokens.list failed".to_string(),
        })?;
    let password = newest_active_token(tokens).ok_or_else(|| {
        AlienError::new(ErrorData::RedisCredentialInvalid {
            binding_name: binding_name.to_string(),
            source_locator: name,
            reason: "the instance's default user has no ACTIVE token".to_string(),
        })
    })?;
    resolve_params(
        binding_name,
        &binding.host,
        &binding.port,
        &binding.username,
        &password,
        tls,
    )
}

/// `(subscription, resource group, cluster name)` from a database resource ID such as
/// `/subscriptions/s/resourceGroups/rg/providers/Microsoft.Cache/redisEnterprise/c/databases/default`.
#[cfg(feature = "azure")]
pub(crate) fn parse_database_id(id: &str) -> Option<(String, String, String)> {
    let parts: Vec<&str> = id.trim_matches('/').split('/').collect();
    let at = |key: &str| {
        parts
            .iter()
            .position(|part| part.eq_ignore_ascii_case(key))
            .and_then(|index| parts.get(index + 1))
            .map(|value| value.to_string())
    };
    if at("providers")?.eq_ignore_ascii_case("Microsoft.Cache") && !at("databases")?.is_empty() {
        Some((
            at("subscriptions")?,
            at("resourceGroups")?,
            at("redisEnterprise")?,
        ))
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
    let (subscription, resource_group, cluster) =
        parse_database_id(&database_id).ok_or_else(|| {
            AlienError::new(ErrorData::config_invalid(
                binding_name,
                "databaseResourceId must be an Azure Managed Redis database resource ID",
            ))
        })?;
    let keys = client
        .list_keys(&subscription, &resource_group, &cluster)
        .await
        .context(ErrorData::RedisCredentialResolutionFailed {
            binding_name: binding_name.to_string(),
            source_locator: database_id.clone(),
            reason: "listKeys failed".to_string(),
        })?;
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
        let id = "/subscriptions/sub-from-id/resourceGroups/my-rg/providers/Microsoft.Cache/redisEnterprise/test-cache/databases/default";
        assert_eq!(
            parse_database_id(id),
            Some((
                "sub-from-id".to_string(),
                "my-rg".to_string(),
                "test-cache".to_string()
            ))
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
        use alien_gcp_clients::memorystore::{AuthToken, MockMemorystoreApi};

        const CA: &str = "-----BEGIN CERTIFICATE-----\nroot\n-----END CERTIFICATE-----";

        fn binding(cas: Vec<String>) -> alien_core::bindings::MemorystoreRedisBinding {
            alien_core::bindings::MemorystoreRedisBinding {
                host: BindingValue::value("10.0.0.5".to_string()),
                port: BindingValue::value(6379),
                username: BindingValue::value("default".to_string()),
                server_ca_certificates: BindingValue::value(cas),
                instance_name: BindingValue::value(
                    "projects/p/locations/us-east4/instances/test-cache".to_string(),
                ),
            }
        }

        fn token(value: &str, state: &str, created: &str) -> AuthToken {
            AuthToken {
                name: format!("t-{value}"),
                token: value.to_string(),
                state: Some(state.to_string()),
                create_time: Some(created.to_string()),
            }
        }

        fn memorystore_returning(tokens: Vec<AuthToken>) -> MockMemorystoreApi {
            let mut memorystore = MockMemorystoreApi::new();
            memorystore
                .expect_list_auth_tokens()
                .times(1)
                .withf(|location, instance, user| {
                    location == "us-east4" && instance == "test-cache" && user == "default"
                })
                .return_once(move |_, _, _| Ok(tokens));
            memorystore
        }

        #[tokio::test]
        async fn reads_the_token_from_the_instance_and_verifies_against_the_binding_cas() {
            let memorystore =
                memorystore_returning(vec![token("token-value", "ACTIVE", "2026-01-01T00:00:00Z")]);
            let params =
                resolve_memorystore("cache", &binding(vec![CA.to_string()]), &memorystore, "p")
                    .await
                    .unwrap();
            assert_eq!(params.password, "token-value");
            assert_eq!(params.tls.ca_certificates(), [CA.to_string()]);
        }

        /// A rotation adds a second token before deleting the first; the newer one is the one
        /// that survives.
        #[tokio::test]
        async fn picks_the_newest_active_token() {
            let memorystore = memorystore_returning(vec![
                token("old", "ACTIVE", "2026-01-01T00:00:00Z"),
                token("new", "ACTIVE", "2026-03-01T00:00:00Z"),
                token("newer-but-inactive", "CREATING", "2026-04-01T00:00:00Z"),
            ]);
            let params =
                resolve_memorystore("cache", &binding(vec![CA.to_string()]), &memorystore, "p")
                    .await
                    .unwrap();
            assert_eq!(params.password, "new");
        }

        /// Fractional digits vary, so `.5Z` vs `.123456Z` must compare as times, not strings.
        #[tokio::test]
        async fn newest_token_compares_timestamps_not_strings() {
            let memorystore = memorystore_returning(vec![
                token("latest", "ACTIVE", "2026-01-01T00:00:00.9Z"),
                token("earlier", "ACTIVE", "2026-01-01T00:00:00.123456Z"),
                token("oldest", "ACTIVE", "2025-12-31T23:59:59.999999999Z"),
            ]);
            let params =
                resolve_memorystore("cache", &binding(vec![CA.to_string()]), &memorystore, "p")
                    .await
                    .unwrap();
            assert_eq!(params.password, "latest");
        }

        #[tokio::test]
        async fn a_binding_for_another_project_is_rejected_before_any_read() {
            let mut memorystore = MockMemorystoreApi::new();
            memorystore.expect_list_auth_tokens().never();
            let error = resolve_memorystore(
                "cache",
                &binding(vec![CA.to_string()]),
                &memorystore,
                "other",
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, "BINDING_CONFIG_INVALID");
        }

        #[tokio::test]
        async fn no_active_token_is_an_invalid_credential() {
            let memorystore =
                memorystore_returning(vec![token("t", "CREATING", "2026-01-01T00:00:00Z")]);
            let error =
                resolve_memorystore("cache", &binding(vec![CA.to_string()]), &memorystore, "p")
                    .await
                    .unwrap_err();
            assert_eq!(error.code, "REDIS_CREDENTIAL_INVALID");
            assert!(!format!("{error:?}").contains("\"t\""));
        }

        #[tokio::test]
        async fn a_denied_read_is_a_resolution_failure_naming_the_instance() {
            let mut memorystore = MockMemorystoreApi::new();
            memorystore.expect_list_auth_tokens().returning(|_, _, _| {
                Err(alien_error::AlienError::new(
                    alien_client_core::ErrorData::RemoteAccessDenied {
                        resource_type: "token".to_string(),
                        resource_name: "default".to_string(),
                    },
                ))
            });
            let error =
                resolve_memorystore("cache", &binding(vec![CA.to_string()]), &memorystore, "p")
                    .await
                    .unwrap_err();
            assert_eq!(error.code, "REDIS_CREDENTIAL_RESOLUTION_FAILED");
        }

        #[tokio::test]
        async fn missing_cas_fail_before_any_token_read() {
            // The mock has no expectation, so a token read would panic.
            let memorystore = MockMemorystoreApi::new();
            let error = resolve_memorystore("cache", &binding(vec![]), &memorystore, "p")
                .await
                .unwrap_err();
            assert_eq!(error.code, "BINDING_CONFIG_INVALID");
        }

        #[tokio::test]
        async fn a_malformed_instance_name_is_refused_before_any_call() {
            let mut bad = binding(vec![CA.to_string()]);
            bad.instance_name = BindingValue::value("test-cache".to_string());
            let error = resolve_memorystore("cache", &bad, &MockMemorystoreApi::new(), "p")
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
                    "/subscriptions/sub-from-id/resourceGroups/my-rg/providers/Microsoft.Cache/redisEnterprise/test-cache/databases/default".to_string(),
                ),
            }
        }

        #[tokio::test]
        async fn lists_the_database_key_in_the_right_group_and_cluster() {
            let mut client = MockManagedRedisApi::new();
            client
                .expect_list_keys()
                .times(1)
                .withf(|subscription, group, cluster| {
                    subscription == "sub-from-id" && group == "my-rg" && cluster == "test-cache"
                })
                .returning(|_, _, _| {
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
            client.expect_list_keys().returning(|_, _, _| {
                Ok(serde_json::from_str::<ManagedRedisAccessKeys>("{}").unwrap())
            });
            let error = resolve_azure_managed_redis("cache", &binding(), &client)
                .await
                .unwrap_err();
            assert_eq!(error.code, "REDIS_CREDENTIAL_INVALID");
        }
    }
}
