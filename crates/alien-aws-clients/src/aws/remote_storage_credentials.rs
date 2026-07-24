use alien_client_core::{ErrorData, Result};
use alien_error::{AlienError, Context, IntoAlienError};

use super::{
    sts::{AssumeRoleRequest, AssumeRoleWithWebIdentityRequest, StsApi, StsClient},
    AwsClientConfig, AwsClientConfigExt, AwsCredentials,
};

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
        credentials: credentials.into(),
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
        credentials: credentials.into(),
        service_overrides: config.service_overrides.clone(),
    })
}
