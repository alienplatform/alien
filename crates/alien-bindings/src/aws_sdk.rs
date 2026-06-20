use alien_core::{AwsClientConfig, AwsCredentials, AwsImpersonationConfig, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use aws_config::{BehaviorVersion, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_codebuild::Client as CodeBuildClient;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_ecr::Client as EcrClient;
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_ssm::Client as SsmClient;
use aws_sdk_sts::{primitives::DateTimeFormat, Client as StsClient};
use aws_types::region::Region;

use crate::error::{ErrorData, Result};

/// Build an official AWS SDK config from Alien's public AWS client config.
pub async fn sdk_config_from_alien_config(config: &AwsClientConfig) -> Result<SdkConfig> {
    let region = Region::new(config.region.clone());
    let loader = aws_config::defaults(BehaviorVersion::latest()).region(region.clone());

    let loader = match &config.credentials {
        AwsCredentials::AccessKeys {
            access_key_id,
            secret_access_key,
            session_token,
        } => loader.credentials_provider(Credentials::new(
            access_key_id,
            secret_access_key,
            session_token.clone(),
            None,
            "AlienAccessKeys",
        )),
        AwsCredentials::SessionCredentials {
            access_key_id,
            secret_access_key,
            session_token,
            expires_at,
        } => {
            let expires_after = chrono::DateTime::parse_from_rfc3339(expires_at)
                .map(|expires_at| expires_at.to_utc().into())
                .into_alien_error()
                .context(ErrorData::ClientConfigInvalid {
                    platform: Platform::Aws,
                    message: format!("Invalid AWS credential expiration timestamp: {expires_at}"),
                })?;

            loader.credentials_provider(Credentials::new(
                access_key_id,
                secret_access_key,
                Some(session_token.clone()),
                Some(expires_after),
                "AlienSessionCredentials",
            ))
        }
        AwsCredentials::Profile { name } => loader.profile_name(name),
        AwsCredentials::WebIdentity { config } => {
            let provider_config = aws_config::provider_config::ProviderConfig::without_region()
                .with_region(Some(region));
            let provider =
                aws_config::web_identity_token::WebIdentityTokenCredentialsProvider::builder()
                    .configure(&provider_config)
                    .static_configuration(aws_config::web_identity_token::StaticConfiguration {
                        web_identity_token_file: config.web_identity_token_file.clone().into(),
                        role_arn: config.role_arn.clone(),
                        session_name: config
                            .session_name
                            .clone()
                            .unwrap_or_else(|| "alien-web-identity".to_string()),
                    })
                    .build();
            loader.credentials_provider(provider)
        }
        AwsCredentials::Imds { endpoint } => {
            let provider_config = aws_config::provider_config::ProviderConfig::without_region()
                .with_region(Some(region));
            let mut client_builder =
                aws_config::imds::Client::builder().configure(&provider_config);
            if let Some(endpoint) = endpoint {
                client_builder = client_builder.endpoint(endpoint).map_err(|err| {
                    AlienError::new(ErrorData::ClientConfigInvalid {
                        platform: Platform::Aws,
                        message: format!("Invalid AWS IMDS endpoint override '{endpoint}': {err}"),
                    })
                })?;
            }
            let imds_client = client_builder.build();
            let provider = aws_config::imds::credentials::ImdsCredentialsProvider::builder()
                .configure(&provider_config)
                .imds_client(imds_client)
                .build();
            loader.credentials_provider(provider)
        }
    };

    Ok(loader.load().await)
}

/// Create an official AWS SDK SSM client with Alien endpoint override support.
pub async fn ssm_client_from_alien_config(config: &AwsClientConfig) -> Result<SsmClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ssm_config = aws_sdk_ssm::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ssm"))
    {
        ssm_config = ssm_config.endpoint_url(endpoint);
    }

    Ok(SsmClient::from_conf(ssm_config.build()))
}

/// Create an official AWS SDK Lambda client with Alien endpoint override support.
pub async fn lambda_client_from_alien_config(config: &AwsClientConfig) -> Result<LambdaClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut lambda_config = aws_sdk_lambda::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("lambda"))
    {
        lambda_config = lambda_config.endpoint_url(endpoint);
    }

    Ok(LambdaClient::from_conf(lambda_config.build()))
}

/// Create an official AWS SDK SQS client with Alien endpoint override support.
pub async fn sqs_client_from_alien_config(config: &AwsClientConfig) -> Result<SqsClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut sqs_config = aws_sdk_sqs::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("sqs"))
    {
        sqs_config = sqs_config.endpoint_url(endpoint);
    }

    Ok(SqsClient::from_conf(sqs_config.build()))
}

/// Create an official AWS SDK DynamoDB client with Alien endpoint override support.
pub async fn dynamodb_client_from_alien_config(config: &AwsClientConfig) -> Result<DynamoDbClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut dynamodb_config = aws_sdk_dynamodb::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("dynamodb"))
    {
        dynamodb_config = dynamodb_config.endpoint_url(endpoint);
    }

    Ok(DynamoDbClient::from_conf(dynamodb_config.build()))
}

/// Create an official AWS SDK STS client with Alien endpoint override support.
pub async fn sts_client_from_alien_config(config: &AwsClientConfig) -> Result<StsClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut sts_config = aws_sdk_sts::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("sts"))
    {
        sts_config = sts_config.endpoint_url(endpoint);
    }

    Ok(StsClient::from_conf(sts_config.build()))
}

/// Assume an AWS IAM role and return a new Alien AWS config backed by temporary credentials.
pub async fn assume_role_config_from_alien_config(
    config: &AwsClientConfig,
    impersonation: AwsImpersonationConfig,
) -> Result<AwsClientConfig> {
    let target_account_id = extract_account_id_from_role_arn(&impersonation.role_arn)
        .unwrap_or_else(|| config.account_id.clone());
    let target_region = impersonation
        .target_region
        .clone()
        .unwrap_or_else(|| config.region.clone());
    let session_name = impersonation
        .session_name
        .as_deref()
        .unwrap_or("alien-impersonation");

    let mut request = sts_client_from_alien_config(config)
        .await?
        .assume_role()
        .role_arn(&impersonation.role_arn)
        .role_session_name(session_name);

    if let Some(duration_seconds) = impersonation.duration_seconds {
        request = request.duration_seconds(duration_seconds);
    }
    if let Some(external_id) = &impersonation.external_id {
        request = request.external_id(external_id);
    }

    let response =
        request
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::ClientConfigInvalid {
                platform: Platform::Aws,
                message: format!("Failed to assume AWS role '{}'", impersonation.role_arn),
            })?;

    let credentials = response.credentials().ok_or_else(|| {
        AlienError::new(ErrorData::ClientConfigInvalid {
            platform: Platform::Aws,
            message: format!(
                "AssumeRole for '{}' returned no credentials",
                impersonation.role_arn
            ),
        })
    })?;

    let expires_at = credentials
        .expiration()
        .fmt(DateTimeFormat::DateTime)
        .into_alien_error()
        .context(ErrorData::ClientConfigInvalid {
            platform: Platform::Aws,
            message: format!(
                "Failed to format AssumeRole credential expiration for '{}'",
                impersonation.role_arn
            ),
        })?;

    Ok(AwsClientConfig {
        account_id: target_account_id,
        region: target_region,
        credentials: AwsCredentials::SessionCredentials {
            access_key_id: credentials.access_key_id().to_string(),
            secret_access_key: credentials.secret_access_key().to_string(),
            session_token: credentials.session_token().to_string(),
            expires_at,
        },
        service_overrides: config.service_overrides.clone(),
    })
}

fn extract_account_id_from_role_arn(role_arn: &str) -> Option<String> {
    let parts: Vec<&str> = role_arn.split(':').collect();
    (parts.len() >= 6 && parts[2] == "iam")
        .then(|| parts[4].to_string())
        .filter(|account_id| !account_id.is_empty())
}

/// Create an official AWS SDK CodeBuild client with Alien endpoint override support.
pub async fn codebuild_client_from_alien_config(
    config: &AwsClientConfig,
) -> Result<CodeBuildClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut codebuild_config = aws_sdk_codebuild::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("codebuild"))
    {
        codebuild_config = codebuild_config.endpoint_url(endpoint);
    }

    Ok(CodeBuildClient::from_conf(codebuild_config.build()))
}

/// Create an official AWS SDK ECR client with Alien endpoint override support.
pub async fn ecr_client_from_alien_config(config: &AwsClientConfig) -> Result<EcrClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ecr_config = aws_sdk_ecr::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ecr"))
    {
        ecr_config = ecr_config.endpoint_url(endpoint);
    }

    Ok(EcrClient::from_conf(ecr_config.build()))
}
