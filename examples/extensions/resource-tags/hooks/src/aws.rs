use alien_extension_sdk::prelude::*;

use crate::TAGS;

/// Adds the tags to every Aurora cluster Alien creates.
pub struct RdsTags;

impl AwsInterceptor for RdsTags {
    fn matches(&self, call: &AwsCall) -> bool {
        call.service() == "rds" && call.operation() == "CreateDBCluster"
    }

    fn modify_request(&self, request: &mut AwsRequest) -> Result<()> {
        for (key, value) in TAGS {
            request.push_member("Tags.Tag", [("Key", *key), ("Value", *value)]);
        }
        Ok(())
    }
}

/// Adds the tags to every DynamoDB table Alien creates.
pub struct DynamoDbTags;

impl AwsInterceptor for DynamoDbTags {
    fn matches(&self, call: &AwsCall) -> bool {
        call.service() == "dynamodb" && call.operation() == "CreateTable"
    }

    fn modify_request(&self, request: &mut AwsRequest) -> Result<()> {
        let body = request.body_mut()?;
        let mut tags = body["Tags"].as_array().cloned().unwrap_or_default();
        tags.extend(TAGS.iter().map(|(key, value)| json!({ "Key": key, "Value": value })));
        body["Tags"] = Value::Array(tags);
        Ok(())
    }
}

#[test]
fn adds_tags_after_alien_tags() {
    let mut request = AwsRequest::new(
        "rds",
        "CreateDBCluster",
        json!({ "DBClusterIdentifier": "db", "Tags.Tag.1.Key": "deployment", "Tags.Tag.1.Value": "stack" }),
    );
    RdsTags.modify_request(&mut request).unwrap();
    assert_eq!(request.param("Tags.Tag.1.Key"), Some("deployment"));
    assert_eq!(request.param("Tags.Tag.2.Key"), Some("team"));
    assert_eq!(request.param("Tags.Tag.3.Value"), Some("acme"));
}
