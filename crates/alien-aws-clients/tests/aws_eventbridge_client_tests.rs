/*!
# EventBridge Client Integration Tests

These tests perform real AWS EventBridge operations to test rule and target management.

## Test Structure

1. **test_put_rule_with_cron_schedule** — Create a rule with a cron schedule expression
2. **test_put_rule_with_rate_schedule** — Create a rule with a rate expression
3. **test_put_targets** — Create a rule and add a Lambda target
4. **test_remove_targets** — Create a rule with targets, then remove them
5. **test_delete_rule** — Create a rule, then delete it
6. **test_delete_nonexistent_rule** — Deleting a missing rule should handle gracefully
7. **test_put_rule_idempotent** — Putting the same rule twice should update it

## Prerequisites

### 1. AWS Credentials
Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=eu-central-1
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
```

### 2. Required Permissions
Your AWS credentials need these permissions:
- `events:PutRule`
- `events:DeleteRule`
- `events:PutTargets`
- `events:RemoveTargets`

## Running Tests
```bash
# Run all EventBridge tests
cargo test --package alien-aws-clients --test aws_eventbridge_client_tests -- --nocapture

# Run specific test
cargo test --package alien-aws-clients --test aws_eventbridge_client_tests test_put_rule_with_cron_schedule -- --nocapture
```

All tests work with real AWS resources and will fail if operations don't succeed.
*/

use alien_aws_clients::aws::eventbridge::*;
use alien_aws_clients::AwsCredentialProvider;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;
use workspace_root;

struct EventBridgeTestContext {
    client: EventBridgeClient,
    account_id: String,
    created_rules: Mutex<HashSet<String>>,
}

impl AsyncTestContext for EventBridgeTestContext {
    async fn setup() -> EventBridgeTestContext {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let region = std::env::var("AWS_MANAGEMENT_REGION")
            .expect("AWS_MANAGEMENT_REGION must be set in .env.test");
        let access_key = std::env::var("AWS_MANAGEMENT_ACCESS_KEY_ID")
            .expect("AWS_MANAGEMENT_ACCESS_KEY_ID must be set in .env.test");
        let secret_key = std::env::var("AWS_MANAGEMENT_SECRET_ACCESS_KEY")
            .expect("AWS_MANAGEMENT_SECRET_ACCESS_KEY must be set in .env.test");
        let account_id = std::env::var("AWS_MANAGEMENT_ACCOUNT_ID")
            .expect("AWS_MANAGEMENT_ACCOUNT_ID must be set in .env.test");

        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id: account_id.clone(),
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };
        let client = EventBridgeClient::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );

        EventBridgeTestContext {
            client,
            account_id,
            created_rules: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        let rule_names: Vec<String> = {
            let created_rules = self.created_rules.lock().unwrap();
            created_rules.iter().cloned().collect()
        };

        for rule_name in rule_names {
            // Remove all targets before deleting — EventBridge requires rules have no targets
            match self
                .client
                .remove_targets(&rule_name, vec!["target-1".to_string()])
                .await
            {
                Ok(_) => info!("Removed targets from rule: {}", rule_name),
                Err(_) => {
                    // Target may not exist, that's fine during cleanup
                }
            }

            match self.client.delete_rule(&rule_name).await {
                Ok(_) => info!("Successfully deleted rule: {}", rule_name),
                Err(e) => {
                    warn!("Failed to delete rule {}: {:?}", rule_name, e);
                }
            }
        }
    }
}

/// Helper to generate a unique rule name for each test to avoid conflicts.
fn test_rule_name(prefix: &str) -> String {
    format!("alien-test-{}-{}", prefix, Uuid::new_v4().simple())
}

#[test_context(EventBridgeTestContext)]
#[tokio::test]
async fn test_put_rule_with_cron_schedule(ctx: &mut EventBridgeTestContext) {
    let rule_name = test_rule_name("cron");
    info!("Testing PutRule with cron schedule: {}", rule_name);

    let request = PutRuleRequest::builder()
        .name(rule_name.clone())
        .schedule_expression("cron(0 12 * * ? *)".to_string())
        .state("DISABLED".to_string())
        .description("Integration test rule with cron schedule".to_string())
        .build();

    let response = ctx
        .client
        .put_rule(request)
        .await
        .expect("PutRule with cron schedule should succeed");

    // Track for cleanup
    {
        let mut created_rules = ctx.created_rules.lock().unwrap();
        created_rules.insert(rule_name.clone());
    }

    let rule_arn = response
        .rule_arn
        .expect("PutRule response should contain a rule ARN");
    assert!(
        rule_arn.contains(&rule_name),
        "Rule ARN '{}' should contain the rule name '{}'",
        rule_arn,
        rule_name
    );
    assert!(
        rule_arn.starts_with("arn:aws:events:"),
        "Rule ARN '{}' should start with 'arn:aws:events:'",
        rule_arn
    );

    info!("Created rule with ARN: {}", rule_arn);
}

#[test_context(EventBridgeTestContext)]
#[tokio::test]
async fn test_put_rule_with_rate_schedule(ctx: &mut EventBridgeTestContext) {
    let rule_name = test_rule_name("rate");
    info!("Testing PutRule with rate schedule: {}", rule_name);

    let request = PutRuleRequest::builder()
        .name(rule_name.clone())
        .schedule_expression("rate(5 minutes)".to_string())
        .state("DISABLED".to_string())
        .description("Integration test rule with rate schedule".to_string())
        .build();

    let response = ctx
        .client
        .put_rule(request)
        .await
        .expect("PutRule with rate schedule should succeed");

    // Track for cleanup
    {
        let mut created_rules = ctx.created_rules.lock().unwrap();
        created_rules.insert(rule_name.clone());
    }

    let rule_arn = response
        .rule_arn
        .expect("PutRule response should contain a rule ARN");
    assert!(
        rule_arn.contains(&rule_name),
        "Rule ARN '{}' should contain the rule name '{}'",
        rule_arn,
        rule_name
    );
    assert!(
        rule_arn.starts_with("arn:aws:events:"),
        "Rule ARN '{}' should start with 'arn:aws:events:'",
        rule_arn
    );

    info!("Created rule with ARN: {}", rule_arn);
}

#[test_context(EventBridgeTestContext)]
#[tokio::test]
async fn test_put_targets(ctx: &mut EventBridgeTestContext) {
    let rule_name = test_rule_name("targets");
    info!("Testing PutTargets: {}", rule_name);

    // First create a rule
    let put_rule_request = PutRuleRequest::builder()
        .name(rule_name.clone())
        .schedule_expression("rate(1 hour)".to_string())
        .state("DISABLED".to_string())
        .description("Integration test rule for PutTargets".to_string())
        .build();

    ctx.client
        .put_rule(put_rule_request)
        .await
        .expect("PutRule should succeed before adding targets");

    // Track for cleanup
    {
        let mut created_rules = ctx.created_rules.lock().unwrap();
        created_rules.insert(rule_name.clone());
    }

    // Add a Lambda target (using a fake but valid-format ARN)
    let fake_lambda_arn = format!(
        "arn:aws:lambda:eu-central-1:{}:function:alien-test-nonexistent",
        ctx.account_id
    );

    let put_targets_request = PutTargetsRequest {
        rule: rule_name.clone(),
        targets: vec![EventBridgeTarget {
            id: "target-1".to_string(),
            arn: fake_lambda_arn.clone(),
        }],
    };

    ctx.client
        .put_targets(put_targets_request)
        .await
        .expect("PutTargets should succeed");

    info!(
        "Successfully added target to rule '{}' with ARN: {}",
        rule_name, fake_lambda_arn
    );
}

#[test_context(EventBridgeTestContext)]
#[tokio::test]
async fn test_remove_targets(ctx: &mut EventBridgeTestContext) {
    let rule_name = test_rule_name("rm-targets");
    info!("Testing RemoveTargets: {}", rule_name);

    // Create rule
    let put_rule_request = PutRuleRequest::builder()
        .name(rule_name.clone())
        .schedule_expression("rate(1 hour)".to_string())
        .state("DISABLED".to_string())
        .description("Integration test rule for RemoveTargets".to_string())
        .build();

    ctx.client
        .put_rule(put_rule_request)
        .await
        .expect("PutRule should succeed");

    // Track for cleanup
    {
        let mut created_rules = ctx.created_rules.lock().unwrap();
        created_rules.insert(rule_name.clone());
    }

    // Add a target
    let fake_lambda_arn = format!(
        "arn:aws:lambda:eu-central-1:{}:function:alien-test-nonexistent",
        ctx.account_id
    );

    let put_targets_request = PutTargetsRequest {
        rule: rule_name.clone(),
        targets: vec![EventBridgeTarget {
            id: "target-1".to_string(),
            arn: fake_lambda_arn,
        }],
    };

    ctx.client
        .put_targets(put_targets_request)
        .await
        .expect("PutTargets should succeed");

    info!("Added target to rule '{}'", rule_name);

    // Remove the target
    ctx.client
        .remove_targets(&rule_name, vec!["target-1".to_string()])
        .await
        .expect("RemoveTargets should succeed");

    info!("Successfully removed target from rule '{}'", rule_name);
}

#[test_context(EventBridgeTestContext)]
#[tokio::test]
async fn test_delete_rule(ctx: &mut EventBridgeTestContext) {
    let rule_name = test_rule_name("delete");
    info!("Testing DeleteRule: {}", rule_name);

    // Create rule
    let put_rule_request = PutRuleRequest::builder()
        .name(rule_name.clone())
        .schedule_expression("rate(1 hour)".to_string())
        .state("DISABLED".to_string())
        .description("Integration test rule for DeleteRule".to_string())
        .build();

    ctx.client
        .put_rule(put_rule_request)
        .await
        .expect("PutRule should succeed");

    info!("Created rule '{}', now deleting it", rule_name);

    // Delete the rule
    ctx.client
        .delete_rule(&rule_name)
        .await
        .expect("DeleteRule should succeed");

    info!("Successfully deleted rule '{}'", rule_name);

    // EventBridge DeleteRule is idempotent — deleting again should also succeed
    ctx.client
        .delete_rule(&rule_name)
        .await
        .expect("DeleteRule is idempotent, second delete should succeed");

    info!(
        "Confirmed DeleteRule is idempotent for rule '{}'",
        rule_name
    );
}

#[test_context(EventBridgeTestContext)]
#[tokio::test]
async fn test_delete_nonexistent_rule(ctx: &mut EventBridgeTestContext) {
    let rule_name = test_rule_name("nonexistent");
    info!(
        "Testing DeleteRule on nonexistent rule: {}",
        rule_name
    );

    // EventBridge DeleteRule is idempotent — succeeds even for non-existent rules
    ctx.client
        .delete_rule(&rule_name)
        .await
        .expect("DeleteRule is idempotent, non-existent rule should succeed");

    info!(
        "Confirmed DeleteRule is idempotent for non-existent rule '{}'",
        rule_name
    );
}

#[test_context(EventBridgeTestContext)]
#[tokio::test]
async fn test_put_rule_idempotent(ctx: &mut EventBridgeTestContext) {
    let rule_name = test_rule_name("idempotent");
    info!("Testing PutRule idempotency: {}", rule_name);

    // Create rule the first time
    let request = PutRuleRequest::builder()
        .name(rule_name.clone())
        .schedule_expression("rate(5 minutes)".to_string())
        .state("DISABLED".to_string())
        .description("Integration test rule — first put".to_string())
        .build();

    let first_response = ctx
        .client
        .put_rule(request)
        .await
        .expect("First PutRule should succeed");

    // Track for cleanup
    {
        let mut created_rules = ctx.created_rules.lock().unwrap();
        created_rules.insert(rule_name.clone());
    }

    let first_arn = first_response
        .rule_arn
        .expect("First PutRule should return a rule ARN");

    info!("First PutRule returned ARN: {}", first_arn);

    // Put the same rule again with a different schedule (update)
    let request = PutRuleRequest::builder()
        .name(rule_name.clone())
        .schedule_expression("rate(10 minutes)".to_string())
        .state("DISABLED".to_string())
        .description("Integration test rule — second put (updated)".to_string())
        .build();

    let second_response = ctx
        .client
        .put_rule(request)
        .await
        .expect("Second PutRule (idempotent update) should succeed");

    let second_arn = second_response
        .rule_arn
        .expect("Second PutRule should return a rule ARN");

    assert_eq!(
        first_arn, second_arn,
        "Putting the same rule twice should return the same ARN"
    );

    info!(
        "Confirmed idempotent PutRule — both calls returned ARN: {}",
        second_arn
    );
}
