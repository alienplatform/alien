use alien_extension_sdk::prelude::*;

mod aws;
mod azure;
mod gcp;
mod kubernetes;

/// The tags every interceptor adds.
pub const TAGS: &[(&str, &str)] = &[("team", "payments"), ("managed-by", "acme")];

fn register(ext: &mut Extension) {
    ext.aws_interceptor(aws::RdsTags);
    ext.aws_interceptor(aws::DynamoDbTags);
    ext.gcp_interceptor(gcp::CloudSqlLabels);
    ext.azure_interceptor(azure::AzureTags);
    ext.kubernetes_interceptor(kubernetes::KubernetesLabels);
}

alien_extension_sdk::export!("tags", register);
