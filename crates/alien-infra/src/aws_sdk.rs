use alien_core::{AwsClientConfig, AwsCredentials, Platform};
use alien_error::{AlienError, Context, ContextError, IntoAlienError, IntoAlienErrorDirect};
use aws_config::{BehaviorVersion, SdkConfig};
use aws_credential_types::Credentials;
use aws_sdk_acm::Client as AcmClient;
use aws_sdk_apigatewayv2::Client as ApiGatewayV2Client;
use aws_sdk_codebuild::Client as CodeBuildClient;
use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_ec2::Client as Ec2Client;
use aws_sdk_ecr::Client as EcrClient;
use aws_sdk_eventbridge::Client as EventBridgeClient;
use aws_sdk_iam::{error::ProvideErrorMetadata, Client as IamClient};
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_sqs::Client as SqsClient;
use aws_sdk_ssm::Client as SsmClient;
use aws_types::region::Region;

use crate::error::{ErrorData, Result};

pub(crate) fn iam_result<T, E>(
    result: std::result::Result<T, aws_sdk_iam::error::SdkError<E>>,
    operation: &str,
    resource_type: &str,
    resource_name: &str,
) -> Result<T>
where
    E: ProvideErrorMetadata + std::error::Error + Send + Sync + 'static,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Some(service_error) = error.as_service_error() {
                match service_error.code() {
                    Some("NoSuchEntity") => {
                        return Err(AlienError::new(ErrorData::CloudResourceNotFound {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                        }));
                    }
                    Some("EntityAlreadyExists") => {
                        return Err(AlienError::new(ErrorData::CloudResourceConflict {
                            resource_type: resource_type.to_string(),
                            resource_name: resource_name.to_string(),
                            message: format!("{operation} reported EntityAlreadyExists"),
                        }));
                    }
                    _ => {}
                }
            }

            Err(error
                .into_alien_error()
                .context(ErrorData::CloudPlatformError {
                    message: format!(
                        "IAM {operation} API failed for {resource_type} '{resource_name}'"
                    ),
                    resource_id: None,
                }))
        }
    }
}

/// Build an official AWS SDK config from Alien's public AWS client config.
pub(crate) async fn sdk_config_from_alien_config(config: &AwsClientConfig) -> Result<SdkConfig> {
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

fn aws_service_endpoint<'a>(config: &'a AwsClientConfig, service_name: &str) -> Option<&'a String> {
    config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get(service_name))
}

macro_rules! official_aws_client_constructor {
    ($(#[$meta:meta])* $fn_name:ident, $client:path, $builder:path, $service_name:literal) => {
        $(#[$meta])*
        pub async fn $fn_name(config: &AwsClientConfig) -> Result<$client> {
            let sdk_config = sdk_config_from_alien_config(config).await?;
            let mut service_config = <$builder>::from(&sdk_config);

            if let Some(endpoint) = aws_service_endpoint(config, $service_name) {
                service_config = service_config.endpoint_url(endpoint);
            }

            Ok(<$client>::from_conf(service_config.build()))
        }
    };
}

official_aws_client_constructor!(
    /// Create an official AWS SDK CodeBuild client with Alien endpoint override support.
    codebuild_client_from_alien_config,
    CodeBuildClient,
    aws_sdk_codebuild::config::Builder,
    "codebuild"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK ACM client with Alien endpoint override support.
    acm_client_from_alien_config,
    AcmClient,
    aws_sdk_acm::config::Builder,
    "acm"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK Lambda client with Alien endpoint override support.
    lambda_client_from_alien_config,
    LambdaClient,
    aws_sdk_lambda::config::Builder,
    "lambda"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK API Gateway V2 client with Alien endpoint override support.
    apigatewayv2_client_from_alien_config,
    ApiGatewayV2Client,
    aws_sdk_apigatewayv2::config::Builder,
    "apigateway"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK EventBridge client with Alien endpoint override support.
    eventbridge_client_from_alien_config,
    EventBridgeClient,
    aws_sdk_eventbridge::config::Builder,
    "events"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK EC2 client with Alien endpoint override support.
    ec2_client_from_alien_config,
    Ec2Client,
    aws_sdk_ec2::config::Builder,
    "ec2"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK ECR client with Alien endpoint override support.
    ecr_client_from_alien_config,
    EcrClient,
    aws_sdk_ecr::config::Builder,
    "ecr"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK IAM client with Alien endpoint override support.
    iam_client_from_alien_config,
    IamClient,
    aws_sdk_iam::config::Builder,
    "iam"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK SSM client with Alien endpoint override support.
    ssm_client_from_alien_config,
    SsmClient,
    aws_sdk_ssm::config::Builder,
    "ssm"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK DynamoDB client with Alien endpoint override support.
    dynamodb_client_from_alien_config,
    DynamoDbClient,
    aws_sdk_dynamodb::config::Builder,
    "dynamodb"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK SQS client with Alien endpoint override support.
    sqs_client_from_alien_config,
    SqsClient,
    aws_sdk_sqs::config::Builder,
    "sqs"
);

official_aws_client_constructor!(
    /// Create an official AWS SDK S3 client with Alien endpoint override support.
    s3_client_from_alien_config,
    S3Client,
    aws_sdk_s3::config::Builder,
    "s3"
);
