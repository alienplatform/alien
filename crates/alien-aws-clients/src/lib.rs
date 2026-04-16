pub mod aws;
pub use aws::*;

// Re-export commonly used types for convenience
pub use aws::credential_provider::AwsCredentialProvider;
pub use aws::{AwsClientConfig, AwsClientConfigExt, AwsImpersonationConfig};

// Re-export all client APIs
pub use aws::acm::{AcmApi, AcmClient};
pub use aws::apigatewayv2::{ApiGatewayV2Api, ApiGatewayV2Client};
pub use aws::cloudformation::{CloudFormationApi, CloudFormationClient};
pub use aws::codebuild::{CodeBuildApi, CodeBuildClient};
pub use aws::dynamodb::{DynamoDbApi, DynamoDbClient};
pub use aws::ec2::{Ec2Api, Ec2Client};
pub use aws::ecr::{EcrApi, EcrClient};
pub use aws::eventbridge::{EventBridgeApi, EventBridgeClient};
pub use aws::iam::{IamApi, IamClient};
pub use aws::lambda::{LambdaApi, LambdaClient};
pub use aws::s3::{S3Api, S3Client};
pub use aws::secrets_manager::{SecretsManagerApi, SecretsManagerClient};
pub use aws::sqs::{SqsApi, SqsClient};
pub use aws::sts::{StsApi, StsClient};

// Re-export error types from alien-client-core
pub use alien_client_core::{Error, ErrorData, Result};
