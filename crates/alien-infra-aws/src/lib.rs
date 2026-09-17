//! AWS infrastructure controllers.

pub mod ai;
pub mod ai_import;
pub mod artifact_registry;
pub mod artifact_registry_import;
pub mod build;
pub mod build_import;
pub mod email;
pub mod email_import;
pub mod key;
pub mod kv;
pub mod kv_import;
pub mod network;
pub mod network_import;
pub mod open_search;
pub mod open_search_import;
pub mod queue;
pub mod queue_import;
mod readiness_probe;
pub mod remote_bindings;
pub mod remote_bindings_import;
pub mod remote_stack_management;
pub mod remote_stack_management_import;
pub mod sandbox;
pub mod sandbox_import;
pub mod service_account;
pub mod service_account_import;
pub mod storage;
pub mod storage_import;
pub mod vault;
pub mod vault_import;
pub mod worker;
pub mod worker_import;

pub mod core {
    pub use crate::{AwsServiceProvider, ResourcePermissionsHelper};
    #[cfg(test)]
    pub use alien_infra::{
        controller_test, deserialize_controller, MockPlatformServiceProvider,
        PlatformServiceProvider, ResourceRegistry, StackExecutor,
    };
    pub use alien_infra_core::*;
}

pub mod error {
    pub use alien_infra_core::{ErrorData, Result};
}

pub mod import {
    pub use alien_infra_core::ResourceImporter;
}

pub mod import_helpers {
    pub use alien_infra_core::{make_imported_state, make_imported_state_with_status};
}

pub use error::Result;

#[cfg(test)]
pub use alien_infra::MockPlatformServiceProvider;
#[cfg(test)]
pub use storage::AwsStorageState;

mod resource_permissions;
pub use resource_permissions::{AwsPermissionsService, ResourcePermissionsHelper};

use std::sync::Arc;

use alien_aws_clients::{
    acm::AcmApi, apigateway::ApiGatewayApi, apigatewayv2::ApiGatewayV2Api,
    autoscaling::AutoScalingApi, bedrock::BedrockApi, cloudformation::CloudFormationApi,
    codebuild::CodeBuildApi, dynamodb::DynamoDbApi, ec2::Ec2Api, ecr::EcrApi, eks::EksApi,
    elbv2::Elbv2Api, eventbridge::EventBridgeApi, iam::IamApi, kms::KmsApi, lambda::LambdaApi,
    lambda_microvms::LambdaMicrovmsApi, rds::RdsApi, s3::S3Api, secrets_manager::SecretsManagerApi,
    ses::SesApi, sqs::SqsApi, ssm::SsmApi, AwsClientConfig,
};

#[async_trait::async_trait]
pub trait AwsServiceProvider: Send + Sync {
    async fn get_aws_iam_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn IamApi>>;
    async fn get_aws_bedrock_client(&self, config: &AwsClientConfig)
        -> Result<Arc<dyn BedrockApi>>;
    async fn get_aws_lambda_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn LambdaApi>>;
    async fn get_aws_microvms_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn LambdaMicrovmsApi>>;
    async fn get_aws_s3_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn S3Api>>;
    async fn get_aws_ses_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn SesApi>>;
    async fn get_aws_cloudformation_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CloudFormationApi>>;
    async fn get_aws_codebuild_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn CodeBuildApi>>;
    async fn get_aws_ecr_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn EcrApi>>;
    async fn get_aws_secrets_manager_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn SecretsManagerApi>>;
    async fn get_aws_rds_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn RdsApi>>;
    async fn get_aws_ssm_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn SsmApi>>;
    async fn get_aws_dynamodb_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn DynamoDbApi>>;
    async fn get_aws_sqs_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn SqsApi>>;
    async fn get_aws_ec2_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn Ec2Api>>;
    async fn get_aws_autoscaling_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn AutoScalingApi>>;
    async fn get_aws_elbv2_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn Elbv2Api>>;
    async fn get_aws_eks_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn EksApi>>;
    async fn get_aws_acm_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn AcmApi>>;
    async fn get_aws_apigateway_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn ApiGatewayApi>>;
    async fn get_aws_apigatewayv2_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn ApiGatewayV2Api>>;
    async fn get_aws_eventbridge_client(
        &self,
        config: &AwsClientConfig,
    ) -> Result<Arc<dyn EventBridgeApi>>;
    async fn get_aws_kms_client(&self, config: &AwsClientConfig) -> Result<Arc<dyn KmsApi>>;
}
