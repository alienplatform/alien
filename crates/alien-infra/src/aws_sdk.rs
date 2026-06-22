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
use aws_sdk_iam::{
    error::ProvideErrorMetadata,
    operation::{
        create_policy::CreatePolicyOutput,
        create_policy_version::CreatePolicyVersionOutput,
        create_role::{CreateRoleInput, CreateRoleOutput},
        get_role::GetRoleOutput,
        get_role_policy::GetRolePolicyOutput,
        list_attached_role_policies::ListAttachedRolePoliciesOutput,
        list_policy_versions::ListPolicyVersionsOutput,
        list_role_policies::ListRolePoliciesOutput,
    },
    Client as IamClient,
};
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_sqs::Client as SqsClient;
use aws_types::region::Region;

use crate::error::{ErrorData, Result};

pub async fn create_iam_role(
    client: &IamClient,
    request: CreateRoleInput,
) -> Result<CreateRoleOutput> {
    let role_name = request
        .role_name()
        .map(ToString::to_string)
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: "CreateRole request did not include roleName".to_string(),
                resource_id: None,
            })
        })?;
    if request.assume_role_policy_document().is_none() {
        return Err(AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "CreateRole request for '{role_name}' did not include assumeRolePolicyDocument"
            ),
            resource_id: None,
        }));
    }

    let response = iam_result(
        client
            .create_role()
            .set_role_name(request.role_name)
            .set_assume_role_policy_document(request.assume_role_policy_document)
            .set_path(request.path)
            .set_description(request.description)
            .set_max_session_duration(request.max_session_duration)
            .set_permissions_boundary(request.permissions_boundary)
            .set_tags(request.tags)
            .send()
            .await,
        "CreateRole",
        "IAM Role",
        &role_name,
    )?;

    response.role().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("IAM CreateRole response for '{role_name}' did not include a role"),
            resource_id: None,
        })
    })?;

    Ok(response)
}

pub async fn get_iam_role(client: &IamClient, role_name: &str) -> Result<GetRoleOutput> {
    let response = iam_result(
        client.get_role().role_name(role_name).send().await,
        "GetRole",
        "IAM Role",
        role_name,
    )?;

    response.role().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("IAM GetRole response for '{role_name}' did not include a role"),
            resource_id: None,
        })
    })?;

    Ok(response)
}

pub async fn delete_iam_role(client: &IamClient, role_name: &str) -> Result<()> {
    iam_result(
        client.delete_role().role_name(role_name).send().await,
        "DeleteRole",
        "IAM Role",
        role_name,
    )?;
    Ok(())
}

pub async fn put_iam_role_policy(
    client: &IamClient,
    role_name: &str,
    policy_name: &str,
    policy_document: &str,
) -> Result<()> {
    let resource_name = format!("{role_name}/{policy_name}");
    iam_result(
        client
            .put_role_policy()
            .role_name(role_name)
            .policy_name(policy_name)
            .policy_document(policy_document)
            .send()
            .await,
        "PutRolePolicy",
        "IAM RolePolicy",
        &resource_name,
    )?;
    Ok(())
}

pub async fn get_iam_role_policy(
    client: &IamClient,
    role_name: &str,
    policy_name: &str,
) -> Result<GetRolePolicyOutput> {
    let resource_name = format!("{role_name}/{policy_name}");
    iam_result(
        client
            .get_role_policy()
            .role_name(role_name)
            .policy_name(policy_name)
            .send()
            .await,
        "GetRolePolicy",
        "IAM RolePolicy",
        &resource_name,
    )
}

pub async fn delete_iam_role_policy(
    client: &IamClient,
    role_name: &str,
    policy_name: &str,
) -> Result<()> {
    let resource_name = format!("{role_name}/{policy_name}");
    iam_result(
        client
            .delete_role_policy()
            .role_name(role_name)
            .policy_name(policy_name)
            .send()
            .await,
        "DeleteRolePolicy",
        "IAM RolePolicy",
        &resource_name,
    )?;
    Ok(())
}

pub async fn update_iam_assume_role_policy(
    client: &IamClient,
    role_name: &str,
    policy_document: &str,
) -> Result<()> {
    iam_result(
        client
            .update_assume_role_policy()
            .role_name(role_name)
            .policy_document(policy_document)
            .send()
            .await,
        "UpdateAssumeRolePolicy",
        "IAM Role",
        role_name,
    )?;
    Ok(())
}

pub async fn list_iam_attached_role_policies(
    client: &IamClient,
    role_name: &str,
) -> Result<ListAttachedRolePoliciesOutput> {
    let response = iam_result(
        client
            .list_attached_role_policies()
            .role_name(role_name)
            .send()
            .await,
        "ListAttachedRolePolicies",
        "IAM Role",
        role_name,
    )?;

    for policy in response.attached_policies() {
        if policy.policy_name().is_none() || policy.policy_arn().is_none() {
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!(
                    "IAM ListAttachedRolePolicies response for '{role_name}' included a policy without both name and ARN"
                ),
                resource_id: None,
            }));
        }
    }

    Ok(response)
}

pub async fn create_iam_policy(
    client: &IamClient,
    policy_name: &str,
    policy_document: &str,
    path: Option<String>,
) -> Result<CreatePolicyOutput> {
    let response = iam_result(
        client
            .create_policy()
            .policy_name(policy_name)
            .policy_document(policy_document)
            .set_path(path)
            .send()
            .await,
        "CreatePolicy",
        "IAM Policy",
        policy_name,
    )?;

    response.policy().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "IAM CreatePolicy response for '{policy_name}' did not include a policy"
            ),
            resource_id: None,
        })
    })?;

    Ok(response)
}

pub async fn delete_iam_policy(client: &IamClient, policy_arn: &str) -> Result<()> {
    iam_result(
        client.delete_policy().policy_arn(policy_arn).send().await,
        "DeletePolicy",
        "IAM Policy",
        policy_arn,
    )?;
    Ok(())
}

pub async fn create_iam_policy_version(
    client: &IamClient,
    policy_arn: &str,
    policy_document: &str,
    set_as_default: bool,
) -> Result<CreatePolicyVersionOutput> {
    let response = iam_result(
        client
            .create_policy_version()
            .policy_arn(policy_arn)
            .policy_document(policy_document)
            .set_as_default(set_as_default)
            .send()
            .await,
        "CreatePolicyVersion",
        "IAM Policy",
        policy_arn,
    )?;

    response.policy_version().ok_or_else(|| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!(
                "IAM CreatePolicyVersion response for '{policy_arn}' did not include a version"
            ),
            resource_id: None,
        })
    })?;

    Ok(response)
}

pub async fn delete_iam_policy_version(
    client: &IamClient,
    policy_arn: &str,
    version_id: &str,
) -> Result<()> {
    let resource_name = format!("{policy_arn}/{version_id}");
    iam_result(
        client
            .delete_policy_version()
            .policy_arn(policy_arn)
            .version_id(version_id)
            .send()
            .await,
        "DeletePolicyVersion",
        "IAM PolicyVersion",
        &resource_name,
    )?;
    Ok(())
}

pub async fn list_iam_policy_versions(
    client: &IamClient,
    policy_arn: &str,
) -> Result<ListPolicyVersionsOutput> {
    iam_result(
        client
            .list_policy_versions()
            .policy_arn(policy_arn)
            .send()
            .await,
        "ListPolicyVersions",
        "IAM Policy",
        policy_arn,
    )
}

pub async fn attach_iam_role_policy(
    client: &IamClient,
    role_name: &str,
    policy_arn: &str,
) -> Result<()> {
    let resource_name = format!("{role_name}/{policy_arn}");
    iam_result(
        client
            .attach_role_policy()
            .role_name(role_name)
            .policy_arn(policy_arn)
            .send()
            .await,
        "AttachRolePolicy",
        "IAM RolePolicyAttachment",
        &resource_name,
    )?;
    Ok(())
}

pub async fn detach_iam_role_policy(
    client: &IamClient,
    role_name: &str,
    policy_arn: &str,
) -> Result<()> {
    let resource_name = format!("{role_name}/{policy_arn}");
    iam_result(
        client
            .detach_role_policy()
            .role_name(role_name)
            .policy_arn(policy_arn)
            .send()
            .await,
        "DetachRolePolicy",
        "IAM RolePolicyAttachment",
        &resource_name,
    )?;
    Ok(())
}

pub async fn list_iam_role_policies(
    client: &IamClient,
    role_name: &str,
) -> Result<ListRolePoliciesOutput> {
    iam_result(
        client
            .list_role_policies()
            .role_name(role_name)
            .send()
            .await,
        "ListRolePolicies",
        "IAM Role",
        role_name,
    )
}

fn iam_result<T, E>(
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

/// Create an official AWS SDK ACM client with Alien endpoint override support.
pub async fn acm_client_from_alien_config(config: &AwsClientConfig) -> Result<AcmClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut acm_config = aws_sdk_acm::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("acm"))
    {
        acm_config = acm_config.endpoint_url(endpoint);
    }

    Ok(AcmClient::from_conf(acm_config.build()))
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

/// Create an official AWS SDK API Gateway V2 client with Alien endpoint override support.
pub async fn apigatewayv2_client_from_alien_config(
    config: &AwsClientConfig,
) -> Result<ApiGatewayV2Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut apigatewayv2_config = aws_sdk_apigatewayv2::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("apigateway"))
    {
        apigatewayv2_config = apigatewayv2_config.endpoint_url(endpoint);
    }

    Ok(ApiGatewayV2Client::from_conf(apigatewayv2_config.build()))
}

/// Create an official AWS SDK EventBridge client with Alien endpoint override support.
pub async fn eventbridge_client_from_alien_config(
    config: &AwsClientConfig,
) -> Result<EventBridgeClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut eventbridge_config = aws_sdk_eventbridge::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("events"))
    {
        eventbridge_config = eventbridge_config.endpoint_url(endpoint);
    }

    Ok(EventBridgeClient::from_conf(eventbridge_config.build()))
}

/// Create an official AWS SDK EC2 client with Alien endpoint override support.
pub async fn ec2_client_from_alien_config(config: &AwsClientConfig) -> Result<Ec2Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ec2_config = aws_sdk_ec2::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ec2"))
    {
        ec2_config = ec2_config.endpoint_url(endpoint);
    }

    Ok(Ec2Client::from_conf(ec2_config.build()))
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

/// Create an official AWS SDK IAM client with Alien endpoint override support.
pub async fn iam_client_from_alien_config(config: &AwsClientConfig) -> Result<IamClient> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut iam_config = aws_sdk_iam::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("iam"))
    {
        iam_config = iam_config.endpoint_url(endpoint);
    }

    Ok(IamClient::from_conf(iam_config.build()))
}

/// Create an official AWS SDK SSM client with Alien endpoint override support.
pub async fn ssm_client_from_alien_config(config: &AwsClientConfig) -> Result<aws_sdk_ssm::Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut ssm_config = aws_sdk_ssm::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("ssm"))
    {
        ssm_config = ssm_config.endpoint_url(endpoint);
    }

    Ok(aws_sdk_ssm::Client::from_conf(ssm_config.build()))
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

/// Create an official AWS SDK S3 client with Alien endpoint override support.
pub async fn s3_client_from_alien_config(config: &AwsClientConfig) -> Result<S3Client> {
    let sdk_config = sdk_config_from_alien_config(config).await?;
    let mut s3_config = aws_sdk_s3::config::Builder::from(&sdk_config);

    if let Some(endpoint) = config
        .service_overrides
        .as_ref()
        .and_then(|overrides| overrides.endpoints.get("s3"))
    {
        s3_config = s3_config.endpoint_url(endpoint);
    }

    Ok(S3Client::from_conf(s3_config.build()))
}
