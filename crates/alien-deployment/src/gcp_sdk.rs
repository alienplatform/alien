use crate::{ErrorData, Result};
use alien_core::{GcpClientConfig, GcpCredentials, GcpImpersonationConfig};
use alien_error::{AlienError, Context, IntoAlienError as _};
use google_cloud_auth::credentials::{
    self, CacheableResource, Credentials, CredentialsProvider, EntityTag,
};
use google_cloud_auth::errors::CredentialsError;
use google_cloud_resourcemanager_v3::client::Projects;
use http::{header::AUTHORIZATION, Extensions, HeaderMap, HeaderValue};
use serde_json::{json, Value};
use std::future::Future;
use std::time::Duration;

const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";

#[derive(Debug, Clone)]
struct StaticAccessTokenCredentials {
    token: String,
    entity_tag: EntityTag,
}

impl StaticAccessTokenCredentials {
    fn new(token: String) -> Self {
        Self {
            token,
            entity_tag: EntityTag::new(),
        }
    }
}

impl CredentialsProvider for StaticAccessTokenCredentials {
    fn headers(
        &self,
        _extensions: Extensions,
    ) -> impl Future<Output = std::result::Result<CacheableResource<HeaderMap>, CredentialsError>> + Send
    {
        let token = self.token.clone();
        let entity_tag = self.entity_tag.clone();
        async move {
            let mut value = HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| CredentialsError::from_source(false, error))?;
            value.set_sensitive(true);

            let mut headers = HeaderMap::new();
            headers.insert(AUTHORIZATION, value);

            Ok(CacheableResource::New {
                entity_tag,
                data: headers,
            })
        }
    }

    fn universe_domain(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
}

pub async fn get_project_number(config: &GcpClientConfig) -> Result<String> {
    let client = resource_manager_projects_client_from_alien_config(config)
        .await
        .context(ErrorData::EnvironmentInfoCollectionFailed {
            platform: "GCP".to_string(),
            reason: "Failed to create official Resource Manager client".to_string(),
        })?;
    let project = client
        .get_project()
        .set_name(format!("projects/{}", config.project_id))
        .send()
        .await
        .into_alien_error()
        .context(ErrorData::EnvironmentInfoCollectionFailed {
            platform: "GCP".to_string(),
            reason: "ResourceManager projects.get failed".to_string(),
        })?;

    project
        .name
        .strip_prefix("projects/")
        .map(str::to_string)
        .ok_or_else(|| {
            AlienError::new(ErrorData::EnvironmentInfoCollectionFailed {
                platform: "GCP".to_string(),
                reason: format!(
                    "ResourceManager returned project name '{}' without projects/ prefix",
                    project.name
                ),
            })
        })
}

async fn resource_manager_projects_client_from_alien_config(
    config: &GcpClientConfig,
) -> Result<Projects> {
    let credentials = credentials_from_alien_config(config)?;
    let mut builder = Projects::builder().with_credentials(credentials);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("resourcemanager"))
    {
        builder = builder.with_endpoint(endpoint.clone());
    }

    builder
        .build()
        .await
        .into_alien_error()
        .context(ErrorData::EnvironmentInfoCollectionFailed {
            platform: "GCP".to_string(),
            reason: "Failed to build official Resource Manager client".to_string(),
        })
}

fn credentials_from_alien_config(config: &GcpClientConfig) -> Result<Credentials> {
    credentials_from_alien_credentials(&config.credentials)
}

fn credentials_from_alien_credentials(credentials: &GcpCredentials) -> Result<Credentials> {
    match credentials {
        GcpCredentials::AccessToken { token } => {
            Ok(Credentials::from(StaticAccessTokenCredentials::new(token.clone())))
        }
        GcpCredentials::ServiceAccountKey { json } => {
            let key = serde_json::from_str::<Value>(json)
                .into_alien_error()
                .context(ErrorData::EnvironmentInfoCollectionFailed {
                    platform: "GCP".to_string(),
                    reason: "Failed to parse service account key JSON".to_string(),
                })?;
            credentials::service_account::Builder::new(key)
                .with_access_specifier(credentials::service_account::AccessSpecifier::from_scopes(
                    [CLOUD_PLATFORM_SCOPE],
                ))
                .build()
                .into_alien_error()
                .context(ErrorData::EnvironmentInfoCollectionFailed {
                    platform: "GCP".to_string(),
                    reason: "Failed to build official service account credentials".to_string(),
                })
        }
        GcpCredentials::ServiceMetadata => credentials::mds::Builder::default()
            .with_scopes([CLOUD_PLATFORM_SCOPE])
            .build()
            .into_alien_error()
            .context(ErrorData::EnvironmentInfoCollectionFailed {
                platform: "GCP".to_string(),
                reason: "Failed to build official metadata server credentials".to_string(),
            }),
        GcpCredentials::ExternalAccount {
            audience,
            subject_token_type,
            token_url,
            credential_source_file,
            service_account_impersonation_url,
        } => {
            let external_account = external_account_json(
                audience,
                subject_token_type,
                token_url,
                credential_source_file,
                service_account_impersonation_url.as_deref(),
            );
            credentials::external_account::Builder::new(external_account)
                .build()
                .into_alien_error()
                .context(ErrorData::EnvironmentInfoCollectionFailed {
                    platform: "GCP".to_string(),
                    reason: "Failed to build official external account credentials".to_string(),
                })
        }
        GcpCredentials::AuthorizedUser {
            client_id,
            client_secret,
            refresh_token,
        } => {
            let authorized_user = json!({
                "type": "authorized_user",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            });
            credentials::user_account::Builder::new(authorized_user)
                .with_scopes([CLOUD_PLATFORM_SCOPE])
                .build()
                .into_alien_error()
                .context(ErrorData::EnvironmentInfoCollectionFailed {
                    platform: "GCP".to_string(),
                    reason: "Failed to build official authorized user credentials".to_string(),
                })
        }
        GcpCredentials::ImpersonatedServiceAccount { source, config } => {
            impersonated_credentials_from_alien_config(source, config)
        }
        GcpCredentials::ProjectedServiceAccount { .. } => {
            Err(AlienError::new(ErrorData::EnvironmentInfoCollectionFailed {
                platform: "GCP".to_string(),
                reason: "Projected service account token files are not a complete official Google auth credential configuration; use external_account credentials with an audience and credential source instead".to_string(),
            }))
        }
    }
}

fn impersonated_credentials_from_alien_config(
    source: &GcpClientConfig,
    config: &GcpImpersonationConfig,
) -> Result<Credentials> {
    let source_credentials = credentials_from_alien_config(source)?;
    let mut builder =
        credentials::impersonated::Builder::from_source_credentials(source_credentials)
            .with_target_principal(config.service_account_email.clone())
            .with_scopes(config.scopes.clone());

    if let Some(delegates) = &config.delegates {
        builder = builder.with_delegates(delegates.clone());
    }

    if let Some(lifetime) = &config.lifetime {
        builder = builder.with_lifetime(parse_google_duration(lifetime)?);
    }

    builder
        .build()
        .into_alien_error()
        .context(ErrorData::EnvironmentInfoCollectionFailed {
            platform: "GCP".to_string(),
            reason: "Failed to build official impersonated service account credentials".to_string(),
        })
}

fn external_account_json(
    audience: &str,
    subject_token_type: &str,
    token_url: &str,
    credential_source_file: &str,
    service_account_impersonation_url: Option<&str>,
) -> Value {
    let mut value = json!({
        "type": "external_account",
        "audience": audience,
        "subject_token_type": subject_token_type,
        "token_url": token_url,
        "credential_source": {
            "file": credential_source_file,
        },
        "scopes": [CLOUD_PLATFORM_SCOPE],
    });

    if let Some(url) = service_account_impersonation_url {
        value["service_account_impersonation_url"] = Value::String(url.to_string());
    }

    value
}

fn parse_google_duration(value: &str) -> Result<Duration> {
    let seconds = value
        .strip_suffix('s')
        .ok_or_else(|| {
            AlienError::new(ErrorData::EnvironmentInfoCollectionFailed {
                platform: "GCP".to_string(),
                reason: format!("Invalid GCP impersonation lifetime '{value}': expected Ns"),
            })
        })?
        .parse::<u64>()
        .into_alien_error()
        .context(ErrorData::EnvironmentInfoCollectionFailed {
            platform: "GCP".to_string(),
            reason: format!("Invalid GCP impersonation lifetime '{value}'"),
        })?;

    Ok(Duration::from_secs(seconds))
}
