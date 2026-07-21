use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};

use super::{
    sts::{AssumeRoleRequest, AssumeRoleWithWebIdentityRequest, StsApi, StsClient},
    AwsClientConfig, AwsClientConfigExt, AwsCredentials,
};

pub(super) async fn materialize_session_credentials(
    config: &AwsClientConfig,
) -> Result<AwsClientConfig> {
    let resolved = config.get_web_identity_credentials().await?;
    match resolved.credentials {
        AwsCredentials::SessionCredentials { .. } => Ok(resolved),
        AwsCredentials::AccessKeys {
            session_token: Some(_),
            ..
        } => Err(AlienError::new(ErrorData::InvalidClientConfig {
            message: "AWS access keys carrying a session token have no authoritative expiry and cannot be reminted with GetSessionToken".to_string(),
            errors: None,
        })),
        AwsCredentials::AccessKeys {
            session_token: None,
            ..
        } => {
            let response = StsClient::new(reqwest::Client::new(), resolved.clone())
                .get_session_token(Some(3600))
                .await?;
            let credentials = response.get_session_token_result.credentials;
            Ok(AwsClientConfig {
                account_id: resolved.account_id,
                region: resolved.region,
                credentials: AwsCredentials::SessionCredentials {
                    access_key_id: credentials.access_key_id,
                    secret_access_key: credentials.secret_access_key,
                    session_token: credentials.session_token,
                    expires_at: credentials.expiration,
                },
                service_overrides: resolved.service_overrides,
            })
        }
        AwsCredentials::Imds { .. }
        | AwsCredentials::Profile { .. }
        | AwsCredentials::WebIdentity { .. } => {
            Err(AlienError::new(ErrorData::InvalidClientConfig {
                message: "AWS credential source did not resolve to session credentials".to_string(),
                errors: None,
            }))
        }
    }
}

pub(super) async fn assume_role_with_session_policy(
    config: &AwsClientConfig,
    role_arn: &str,
    role_session_name: &str,
    duration_seconds: i32,
    policy: &str,
    target_account_id: &str,
    target_region: &str,
) -> Result<AwsClientConfig> {
    let source = config.get_web_identity_credentials().await?;
    let response = StsClient::new(reqwest::Client::new(), source.clone())
        .assume_role(
            AssumeRoleRequest::builder()
                .role_arn(role_arn.to_string())
                .role_session_name(role_session_name.to_string())
                .duration_seconds(duration_seconds)
                .policy(policy.to_string())
                .build(),
        )
        .await?;
    let credentials = response.assume_role_result.credentials;
    Ok(AwsClientConfig {
        account_id: target_account_id.to_string(),
        region: target_region.to_string(),
        credentials: AwsCredentials::SessionCredentials {
            access_key_id: credentials.access_key_id,
            secret_access_key: credentials.secret_access_key,
            session_token: credentials.session_token,
            expires_at: credentials.expiration,
        },
        service_overrides: source.service_overrides,
    })
}

pub(super) async fn materialize_web_identity_session_with_policy(
    config: &AwsClientConfig,
    role_session_name: &str,
    duration_seconds: i32,
    policy: &str,
) -> Result<AwsClientConfig> {
    let AwsCredentials::WebIdentity {
        config: web_identity,
    } = &config.credentials
    else {
        return Err(AlienError::new(ErrorData::InvalidClientConfig {
            message: "AWS remote Storage attenuation requires a web-identity source or an explicit target-role handoff".to_string(),
            errors: None,
        }));
    };
    let token = std::fs::read_to_string(&web_identity.web_identity_token_file)
        .into_alien_error()
        .context(ErrorData::InvalidClientConfig {
            message: format!(
                "Failed to read web identity token file: {}",
                web_identity.web_identity_token_file
            ),
            errors: None,
        })?
        .trim()
        .to_string();
    let unsigned_config = AwsClientConfig {
        account_id: config.account_id.clone(),
        region: config.region.clone(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "UNSIGNED_WEB_IDENTITY".to_string(),
            secret_access_key: "UNSIGNED_WEB_IDENTITY".to_string(),
            session_token: None,
        },
        service_overrides: config.service_overrides.clone(),
    };
    let response = StsClient::new(reqwest::Client::new(), unsigned_config)
        .assume_role_with_web_identity(
            AssumeRoleWithWebIdentityRequest::builder()
                .role_arn(web_identity.role_arn.clone())
                .role_session_name(role_session_name.to_string())
                .web_identity_token(token)
                .duration_seconds(duration_seconds)
                .policy(policy.to_string())
                .build(),
        )
        .await?;
    let credentials = response.assume_role_with_web_identity_result.credentials;
    Ok(AwsClientConfig {
        account_id: config.account_id.clone(),
        region: config.region.clone(),
        credentials: AwsCredentials::SessionCredentials {
            access_key_id: credentials.access_key_id,
            secret_access_key: credentials.secret_access_key,
            session_token: credentials.session_token,
            expires_at: credentials.expiration,
        },
        service_overrides: config.service_overrides.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn materialization_rejects_access_keys_with_an_unknown_expiry() {
        let config = AwsClientConfig {
            account_id: "123456789012".to_string(),
            region: "us-east-1".to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "AKIAUNKNOWNEXPIRY".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: Some("SESSION_TOKEN_MUST_NOT_LEAK".to_string()),
            },
            service_overrides: None,
        };

        let error = materialize_session_credentials(&config)
            .await
            .expect_err("credentials without an authoritative expiry must fail closed");
        let serialized = serde_json::to_string(&error).expect("serialize error");

        assert!(serialized.contains("no authoritative expiry"));
        assert!(!serialized.contains("SESSION_TOKEN_MUST_NOT_LEAK"));
    }
}
