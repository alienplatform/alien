use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::Result;
use crate::ResourceController;
use alien_aws_clients::AwsClientConfig;
use alien_core::Resource;

/// Context provided to CloudFormation state importers
#[derive(Clone)]
pub struct CloudFormationImportContext {
    /// Map of CloudFormation logical ID -> physical ID
    pub cfn_resources: HashMap<String, String>,
    /// AWS platform configuration for making API calls
    pub aws_config: AwsClientConfig,
    /// Stack prefix used during import
    pub resource_prefix: String,
    /// Stack name
    pub stack_name: String,
}

/// Trait for importing state from deployed CloudFormation resources
#[async_trait]
pub trait CloudFormationResourceImporter: Send + Sync + std::fmt::Debug {
    /// Import state from deployed CloudFormation resources
    async fn import_cloudformation_state(
        &self,
        resource: &Resource,
        context: &CloudFormationImportContext,
    ) -> Result<Box<dyn ResourceController>>;
}
