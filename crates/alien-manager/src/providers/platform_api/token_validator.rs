use crate::traits::{AuthSubject, AuthValidator, TokenScope, TokenType};
use alien_error::{AlienError, IntoAlienError};
use alien_platform_api::types::{ServiceAccountSubject, Subject, SubjectScope};
use async_trait::async_trait;
use axum::http;
use reqwest::Client as HttpClient;

/// Validates Bearer tokens by forwarding to `/v1/whoami` on the Platform API.
pub struct PlatformTokenValidator {
    alien_api_url: String,
    http_client: HttpClient,
}

impl PlatformTokenValidator {
    pub fn new(alien_api_url: String) -> Self {
        Self {
            alien_api_url,
            http_client: HttpClient::new(),
        }
    }
}

#[async_trait]
impl AuthValidator for PlatformTokenValidator {
    async fn validate(&self, headers: &http::HeaderMap) -> Result<Option<AuthSubject>, AlienError> {
        let auth_header = match headers.get(http::header::AUTHORIZATION) {
            Some(h) => h,
            None => return Ok(None),
        };

        let auth_value = auth_header.to_str().into_alien_error()?;

        let response = self
            .http_client
            .get(format!("{}/v1/whoami", self.alien_api_url))
            .header("Authorization", auth_value)
            .send()
            .await
            .into_alien_error()?;

        if !response.status().is_success() {
            return Err(AlienError::new(alien_error::GenericError {
                message: format!(
                    "Platform API authentication failed with status {}",
                    response.status()
                ),
            }));
        }

        let subject: Subject = response.json().await.into_alien_error()?;
        Ok(Some(subject_to_auth_subject(subject)))
    }
}

fn subject_to_auth_subject(subject: Subject) -> AuthSubject {
    match subject {
        Subject::UserSubject(user) => AuthSubject {
            token_id: user.id,
            scope: TokenScope {
                token_type: TokenType::Admin,
                deployment_group_id: None,
                deployment_id: None,
            },
        },
        Subject::ServiceAccountSubject(ServiceAccountSubject { id, scope, .. }) => match scope {
            SubjectScope::Workspace | SubjectScope::Manager { .. } => AuthSubject {
                token_id: id,
                scope: TokenScope {
                    token_type: TokenType::Admin,
                    deployment_group_id: None,
                    deployment_id: None,
                },
            },
            SubjectScope::Project { project_id } => AuthSubject {
                token_id: id,
                scope: TokenScope {
                    token_type: TokenType::DeploymentGroup,
                    deployment_group_id: Some(project_id),
                    deployment_id: None,
                },
            },
            SubjectScope::DeploymentGroup {
                deployment_group_id,
                ..
            } => AuthSubject {
                token_id: id,
                scope: TokenScope {
                    token_type: TokenType::DeploymentGroup,
                    deployment_group_id: Some(deployment_group_id),
                    deployment_id: None,
                },
            },
            SubjectScope::Deployment {
                deployment_id,
                project_id,
            } => AuthSubject {
                token_id: id,
                scope: TokenScope {
                    token_type: TokenType::Deployment,
                    deployment_group_id: Some(project_id),
                    deployment_id: Some(deployment_id),
                },
            },
        },
    }
}
