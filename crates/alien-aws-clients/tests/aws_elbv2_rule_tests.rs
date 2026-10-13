//! Local Query/XML contracts; no cloud credentials or resources are used.
use std::collections::HashMap;

use alien_aws_clients::{
    ServiceOverrides,
    credential_provider::AwsCredentialProvider,
    elbv2::{
        Action, CreateRuleRequest, DescribeRulesRequest, ElbTag, Elbv2Api, Elbv2Client,
        FixedResponseActionConfig, ModifyListenerRequest, HostHeaderConditionConfig, ModifyRuleRequest, Rewrite, RewriteConfig, Rewrites,
        RuleCondition, RuleTransform, RuleValues,
    },
};
use alien_core::{AwsClientConfig, AwsCredentials};
use httpmock::prelude::*;

fn client(server: &MockServer) -> Elbv2Client {
    Elbv2Client::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "111122223333".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test".into(),
                secret_access_key: "test".into(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("elasticloadbalancing".into(), server.base_url())]),
            }),
        }),
    )
}

fn pair(key: &str, value: &str) -> String {
    form_urlencoded::Serializer::new(String::new())
        .append_pair(key, value)
        .finish()
}

fn rule_xml(operation: &str) -> String {
    format!(
        r#"<{operation}Response><{operation}Result><Rules><member>
    <RuleArn>owned-rule</RuleArn><Priority>1</Priority><IsDefault>false</IsDefault>
    <Conditions><member><Field>host-header</Field><HostHeaderConfig><Values><member>private.example.test</member><member>*.private.example.test</member></Values></HostHeaderConfig></member></Conditions>
    <Actions><member><Type>forward</Type><TargetGroupArn>owned-target</TargetGroupArn></member></Actions>
    <Transforms><member><Type>url-rewrite</Type><UrlRewriteConfig><Rewrites><member><Regex>^/(.*)$</Regex><Replace>/route/$1?x=a&amp;y=b</Replace></member></Rewrites></UrlRewriteConfig></member></Transforms>
    </member></Rules><NextMarker>next+page</NextMarker></{operation}Result></{operation}Response>"#
    )
}

fn create_request() -> CreateRuleRequest {
    CreateRuleRequest::builder()
        .listener_arn("owned-listener".into())
        .priority(1)
        .conditions(vec![
            RuleCondition::builder()
                .field("host-header".into())
                .host_header_config(
                    HostHeaderConditionConfig::builder()
                        .values(RuleValues {
                            members: vec![
                                "private.example.test".into(),
                                "*.private.example.test".into(),
                            ],
                        })
                        .build(),
                )
                .build(),
        ])
        .actions(vec![
            Action::builder()
                .action_type("forward".into())
                .target_group_arn("owned-target".into())
                .build(),
        ])
        .transforms(vec![
            RuleTransform::builder()
                .transform_type("url-rewrite".into())
                .url_rewrite_config(RewriteConfig {
                    rewrites: Rewrites {
                        members: vec![Rewrite {
                            regex: "^/(.*)$".into(),
                            replace: "/route/$1?x=a&y=b".into(),
                        }],
                    },
                })
                .build(),
        ])
        .tags(vec![ElbTag {
            key: "owner".into(),
            value: "generation".into(),
        }])
        .build()
}

#[tokio::test]
async fn creates_tagged_host_rule_and_round_trips_exact_rewrite() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_contains(pair("Action", "CreateRule"))
                .body_contains(pair("Version", "2015-12-01"))
                .body_contains(pair("ListenerArn", "owned-listener"))
                .body_contains(pair("Priority", "1"))
                .body_contains(pair("Conditions.member.1.Field", "host-header"))
                .body_contains(pair(
                    "Conditions.member.1.HostHeaderConfig.Values.member.2",
                    "*.private.example.test",
                ))
                .body_contains(pair("Actions.member.1.Type", "forward"))
                .body_contains(pair("Actions.member.1.TargetGroupArn", "owned-target"))
                .body_contains(pair("Transforms.member.1.Type", "url-rewrite"))
                .body_contains(pair(
                    "Transforms.member.1.UrlRewriteConfig.Rewrites.member.1.Regex",
                    "^/(.*)$",
                ))
                .body_contains(pair(
                    "Transforms.member.1.UrlRewriteConfig.Rewrites.member.1.Replace",
                    "/route/$1?x=a&y=b",
                ))
                .body_contains(pair("Tags.member.1.Key", "owner"))
                .body_contains(pair("Tags.member.1.Value", "generation"));
            then.status(200).body(rule_xml("CreateRule"));
        })
        .await;
    let response = client(&server).create_rule(create_request()).await.unwrap();
    let rules = response.create_rule_result.rules.members;
    assert_eq!(rules.len(), 1);
    let rule = &rules[0];
    assert_eq!(rule.rule_arn, "owned-rule");
    assert_eq!(rule.priority, "1");
    assert!(!rule.is_default);
    assert_eq!(
        rule.conditions.members[0]
            .host_header_config
            .as_ref()
            .unwrap()
            .values
            .members,
        ["private.example.test", "*.private.example.test"]
    );
    assert_eq!(
        rule.actions.members[0].target_group_arn.as_deref(),
        Some("owned-target")
    );
    assert_eq!(
        rule.transforms.members[0]
            .url_rewrite_config
            .as_ref()
            .unwrap()
            .rewrites
            .members[0]
            .replace,
        "/route/$1?x=a&y=b"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn reads_page_updates_rewrite_and_explicitly_resets_it() {
    let server = MockServer::start_async().await;
    let client = client(&server);
    let read = server
        .mock_async(|when, then| {
            when.method(POST)
                .body_contains(pair("Action", "DescribeRules"))
                .body_contains(pair("RuleArns.member.1", "owned-rule"))
                .body_contains(pair("Marker", "next+page"))
                .body_contains(pair("PageSize", "20"));
            then.status(200).body(rule_xml("DescribeRules"));
        })
        .await;
    let page = client
        .describe_rules(
            DescribeRulesRequest::builder()
                .rule_arns(vec!["owned-rule".into()])
                .marker("next+page".into())
                .page_size(20)
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(
        page.describe_rules_result.next_marker.as_deref(),
        Some("next+page")
    );
    read.assert_async().await;
    let update = server
        .mock_async(|when, then| {
            when.method(POST)
                .body_contains(pair("Action", "ModifyRule"))
                .body_contains(pair("RuleArn", "owned-rule"))
                .body_contains(pair(
                    "Transforms.member.1.UrlRewriteConfig.Rewrites.member.1.Replace",
                    "/route/$1?x=a&y=b",
                ));
            then.status(200).body(rule_xml("ModifyRule"));
        })
        .await;
    let updated = client
        .modify_rule(
            ModifyRuleRequest::builder()
                .rule_arn("owned-rule".into())
                .transforms(create_request().transforms)
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(
        updated.modify_rule_result.rules.members[0].rule_arn,
        "owned-rule"
    );
    update.assert_async().await;
    let reset = server.mock_async(|when, then| {
        when.method(POST).body_contains(pair("Action", "ModifyRule"))
            .body_contains(pair("RuleArn", "owned-rule")).body_contains(pair("ResetTransforms", "true"));
        then.status(200).body("<ModifyRuleResponse><ModifyRuleResult><Rules><member><RuleArn>owned-rule</RuleArn><Priority>1</Priority><IsDefault>false</IsDefault><Conditions/><Actions/></member></Rules></ModifyRuleResult></ModifyRuleResponse>");
    }).await;
    let cleared = client
        .modify_rule(
            ModifyRuleRequest::builder()
                .rule_arn("owned-rule".into())
                .reset_transforms(true)
                .build(),
        )
        .await
        .unwrap();
    assert!(
        cleared.modify_rule_result.rules.members[0]
            .transforms
            .members
            .is_empty()
    );
    reset.assert_async().await;
}

#[tokio::test]
async fn failed_mutations_are_not_retried_and_missing_rule_is_typed() {
    for (operation, code, status) in [
        ("CreateRule", "PriorityInUse", 400),
        ("ModifyRule", "AccessDenied", 403),
        ("DeleteRule", "RuleNotFound", 400),
    ] {
        let server = MockServer::start_async().await;
        let mock = server.mock_async(|when, then| {
            when.method(POST).body_contains(pair("Action", operation));
            then.status(status).body(format!("<ErrorResponse><Error><Code>{code}</Code><Message>rejected</Message></Error></ErrorResponse>"));
        }).await;
        let client = client(&server);
        let error = match operation {
            "CreateRule" => client.create_rule(create_request()).await.unwrap_err(),
            "ModifyRule" => client
                .modify_rule(
                    ModifyRuleRequest::builder()
                        .rule_arn("owned-rule".into())
                        .reset_transforms(true)
                        .build(),
                )
                .await
                .unwrap_err(),
            _ => client.delete_rule("owned-rule").await.unwrap_err(),
        };
        if operation == "DeleteRule" {
            assert!(
                matches!(error.error, Some(alien_client_core::ErrorData::RemoteResourceNotFound { resource_type, .. }) if resource_type == "Rule")
            );
        }
        mock.assert_hits_async(1).await;
    }
}

#[tokio::test]
async fn listener_update_sends_the_complete_default_denial_action() {
    let server = MockServer::start_async().await;
    let mock = server.mock_async(|when, then| {
        when.method(POST).body_contains(pair("Action", "ModifyListener"))
            .body_contains(pair("ListenerArn", "owned-listener"))
            .body_contains(pair("DefaultActions.member.1.Type", "fixed-response"))
            .body_contains(pair("DefaultActions.member.1.Order", "1"))
            .body_contains(pair("DefaultActions.member.1.FixedResponseConfig.StatusCode", "404"))
            .body_contains(pair("DefaultActions.member.1.FixedResponseConfig.ContentType", "text/plain"))
            .body_contains(pair("DefaultActions.member.1.FixedResponseConfig.MessageBody", "Unknown host"));
        then.status(200).body("<ModifyListenerResponse><ModifyListenerResult><Listeners/></ModifyListenerResult></ModifyListenerResponse>");
    }).await;
    client(&server).modify_listener(ModifyListenerRequest::builder().listener_arn("owned-listener".into())
        .default_actions(vec![Action::builder().action_type("fixed-response".into()).order(1)
            .fixed_response_config(FixedResponseActionConfig::builder().status_code("404".into()).content_type("text/plain".into()).message_body("Unknown host".into()).build()).build()]).build()).await.unwrap();
    mock.assert_async().await;
}
