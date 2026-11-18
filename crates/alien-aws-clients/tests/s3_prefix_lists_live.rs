//! Every row of the checked-in S3 prefix-list table that persistent storage setup renders
//! matches what EC2 reports in that region.

use alien_aws_clients::ec2::{Ec2Api, Ec2Client, Filter};
use alien_aws_clients::AwsCredentialProvider;
use alien_core::sandbox_storage_network::S3_PREFIX_LISTS;
use alien_core::{AwsClientConfig, AwsCredentials};

fn env(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("{name} must be set"))
}

#[tokio::test]
#[ignore = "needs live AWS credentials"]
async fn the_s3_prefix_list_table_matches_every_region() {
    for (region, expected) in S3_PREFIX_LISTS {
        let config = AwsClientConfig {
            account_id: env("AWS_TARGET_ACCOUNT_ID"),
            region: region.to_string(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: env("AWS_TARGET_ACCESS_KEY_ID"),
                secret_access_key: env("AWS_TARGET_SECRET_ACCESS_KEY"),
                session_token: std::env::var("AWS_TARGET_SESSION_TOKEN")
                    .ok()
                    .filter(|token| !token.is_empty()),
            },
            service_overrides: None,
        };
        let client = Ec2Client::new(
            reqwest::Client::new(),
            AwsCredentialProvider::from_config_sync(config),
        );
        let lists = client
            .describe_managed_prefix_lists(vec![Filter {
                name: "prefix-list-name".to_string(),
                values: vec![format!("com.amazonaws.{region}.s3")],
            }])
            .await
            .unwrap_or_else(|e| panic!("{region}: {e}"));
        let ids: Vec<String> = lists
            .prefix_list_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|list| list.prefix_list_id)
            .collect();
        assert_eq!(ids, vec![expected.to_string()], "{region}");
    }
}
