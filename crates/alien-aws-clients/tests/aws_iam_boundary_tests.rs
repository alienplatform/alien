use std::collections::HashMap;

use alien_aws_clients::{
    iam::{CreateRoleRequest, IamApi, IamClient, IamRoleWriter, MockIamApi},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use alien_client_core::ErrorData;
use alien_error::AlienError;
use httpmock::prelude::*;
use mockall::{predicate::eq, Sequence};
use serde_json::json;

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/test-deployment-management-boundary";

fn expect_role(iam: &mut MockIamApi, boundary: Option<&'static str>) {
    iam.expect_get_role()
        .with(eq("test-role"))
        .once()
        .returning(move |_| {
            let mut role = json!({"Path": "/", "RoleName": "test-role", "RoleId": "id",
            "Arn": "arn:aws:iam::123456789012:role/test-role", "CreateDate": "2026-01-01"});
            role["Tags"] = json!({"member": [
                {"Key": "deployment", "Value": "test"},
                {"Key": "resource", "Value": "worker"},
                {"Key": "managed-by", "Value": "runtime"}
            ]});
            if let Some(boundary) = boundary {
                role["PermissionsBoundary"] = json!({"PermissionsBoundaryArn": boundary});
            }
            Ok(serde_json::from_value(json!({"GetRoleResult": {"Role": role}})).unwrap())
        });
}

fn expect_policies(iam: &mut MockIamApi, result: serde_json::Value) {
    iam.expect_list_attached_role_policies()
        .with(eq("test-role"))
        .once()
        .return_once(move |_| {
            Ok(serde_json::from_value(json!({"ListAttachedRolePoliciesResult": result})).unwrap())
        });
}

#[tokio::test]
async fn adoption_completes_before_policy_write() {
    let mut iam = MockIamApi::new();
    expect_role(&mut iam, None);
    expect_policies(
        &mut iam,
        json!({"AttachedPolicies": {"member": [{"PolicyName": "runtime", "PolicyArn": "arn:aws:iam::123456789012:policy/runtime"}]}, "IsTruncated": false}),
    );
    let mut sequence = Sequence::new();
    iam.expect_put_role_permissions_boundary()
        .with(eq("test-role"), eq(BOUNDARY))
        .once()
        .in_sequence(&mut sequence)
        .returning(|_, _| Ok(()));
    iam.expect_put_role_policy()
        .with(eq("test-role"), eq("runtime"), eq("{}"))
        .once()
        .in_sequence(&mut sequence)
        .returning(|_, _, _| Ok(()));
    IamRoleWriter::new(&iam, Some(BOUNDARY))
        .prepare_existing("test-role", "test", "worker")
        .await
        .unwrap();
    iam.put_role_policy("test-role", "runtime", "{}")
        .await
        .unwrap();
}

#[tokio::test]
async fn legacy_setup_does_not_read_or_adopt_roles() {
    let mut iam = MockIamApi::new();
    iam.expect_put_role_policy()
        .with(eq("test-role"), eq("runtime"), eq("{}"))
        .once()
        .returning(|_, _, _| Ok(()));
    IamRoleWriter::new(&iam, None)
        .prepare_existing("test-role", "test", "worker")
        .await
        .unwrap();
    iam.put_role_policy("test-role", "runtime", "{}")
        .await
        .unwrap();
}

#[tokio::test]
async fn already_bounded_role_needs_no_adoption() {
    let mut iam = MockIamApi::new();
    expect_role(&mut iam, Some(BOUNDARY));
    IamRoleWriter::new(&iam, Some(BOUNDARY))
        .prepare_existing("test-role", "test", "worker")
        .await
        .unwrap();
}

#[tokio::test]
async fn roles_without_current_runtime_ownership_are_never_written_or_adopted() {
    for boundary in [None, Some(BOUNDARY)] {
        for tags in [
            json!(null),
            json!({"member": []}),
            // Historical tags are not proof of current runtime ownership.
            // These are the values emitted by tags.rs at 61b149801 and 21f84db6c.
            json!({"member": [
                {"Key": "alien.dev/stack", "Value": "test"},
                {"Key": "alien.dev/resource", "Value": "worker"},
                {"Key": "alien.dev/managed-by", "Value": "alien"}
            ]}),
            json!({"member": [
                {"Key": "deployment", "Value": "test"},
                {"Key": "resource", "Value": "worker"},
                {"Key": "managed-by", "Value": "deployment"}
            ]}),
            json!({"member": [
                {"Key": "deployment", "Value": "test-sibling"},
                {"Key": "resource", "Value": "worker"},
                {"Key": "managed-by", "Value": "runtime"}
            ]}),
            json!({"member": [
                {"Key": "deployment", "Value": "test"},
                {"Key": "resource", "Value": "other"},
                {"Key": "managed-by", "Value": "runtime"}
            ]}),
            json!({"member": [
                {"Key": "deployment", "Value": "test"},
                {"Key": "resource", "Value": "worker"},
                {"Key": "managed-by", "Value": "setup"}
            ]}),
            json!({"member": [
                {"Key": "deployment", "Value": "test"},
                {"Key": "resource", "Value": "worker"}
            ]}),
        ] {
            let mut iam = MockIamApi::new();
            iam.expect_get_role().once().return_once(move |_| {
                Ok(serde_json::from_value(json!({"GetRoleResult": {"Role": {
                    "Path": "/", "RoleName": "test-role", "RoleId": "id",
                    "Arn": "arn:aws:iam::123456789012:role/test-role",
                    "CreateDate": "2026-01-01", "Tags": tags,
                    "PermissionsBoundary": boundary.map(|arn| json!({"PermissionsBoundaryArn": arn}))
                }}})).unwrap())
            });
            // No policy audit, adoption, or write may follow a failed ownership check.
            let error = IamRoleWriter::new(&iam, Some(BOUNDARY))
                .prepare_existing("test-role", "test", "worker")
                .await
                .unwrap_err();
            assert!(matches!(
                error.error,
                Some(ErrorData::ExistingCloudResourceNotOwned { .. })
            ));
            assert!(!error.retryable);
        }
    }
}

#[tokio::test]
async fn different_boundary_is_not_overwritten() {
    let mut iam = MockIamApi::new();
    expect_role(&mut iam, Some("arn:aws:iam::123456789012:policy/other"));
    let error = IamRoleWriter::new(&iam, Some(BOUNDARY))
        .prepare_existing("test-role", "test", "worker")
        .await
        .unwrap_err();
    assert!(matches!(error.error, Some(ErrorData::InvalidInput { .. })));
    assert!(error.to_string().contains("different permissions boundary"));
}

#[tokio::test]
async fn unsafe_or_incomplete_policy_audit_prevents_adoption() {
    for (result, diagnostic) in [
        (
            json!({"AttachedPolicies": {"member": [{"PolicyName": "AdministratorAccess", "PolicyArn": "arn:aws:iam::aws:policy/AdministratorAccess"}]}, "IsTruncated": false}),
            "AWS-managed policy",
        ),
        (json!({"IsTruncated": true, "Marker": "next"}), "truncated"),
        (json!({}), "not confirmed complete"),
        (
            json!({"AttachedPolicies": {"member": []}}),
            "not confirmed complete",
        ),
    ] {
        let mut iam = MockIamApi::new();
        expect_role(&mut iam, None);
        expect_policies(&mut iam, result);
        iam.expect_put_role_permissions_boundary().never();
        let error = IamRoleWriter::new(&iam, Some(BOUNDARY))
            .prepare_existing("test-role", "test", "worker")
            .await
            .unwrap_err();
        assert!(matches!(error.error, Some(ErrorData::InvalidInput { .. })));
        assert!(error.to_string().contains(diagnostic));
    }
}

#[tokio::test]
async fn adoption_failure_is_returned_to_the_writer() {
    let mut iam = MockIamApi::new();
    expect_role(&mut iam, None);
    expect_policies(&mut iam, json!({"IsTruncated": false}));
    iam.expect_put_role_permissions_boundary()
        .once()
        .returning(|_, _| {
            Err(AlienError::new(ErrorData::InvalidInput {
                message: "adoption denied".into(),
                field_name: None,
            }))
        });
    let error = IamRoleWriter::new(&iam, Some(BOUNDARY))
        .prepare_existing("test-role", "test", "worker")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("adoption denied"));
}

fn client(server: &MockServer) -> IamClient {
    IamClient::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-key".into(),
                secret_access_key: "test-secret".into(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("iam".into(), server.base_url())]),
            }),
        }),
    )
}

#[tokio::test]
async fn create_sends_boundary_only_when_configured() {
    for boundary in [Some(BOUNDARY), None] {
        let server = MockServer::start_async().await;
        let expected =
            boundary.map(|b| form_urlencoded::byte_serialize(b.as_bytes()).collect::<String>());
        let mock = server.mock_async(|when, then| {
            let when = when.method(POST).header_exists("authorization")
                .body_contains("Action=CreateRole").body_contains("RoleName=test-role");
            if let Some(expected) = expected {
                when.body_contains(format!("PermissionsBoundary={expected}"));
            } else {
                when.matches(|request| {
                    !String::from_utf8_lossy(request.body.as_deref().unwrap()).contains("PermissionsBoundary")
                });
            }
            then.status(200).body("<CreateRoleResponse><CreateRoleResult><Role><Path>/</Path><RoleName>test-role</RoleName><RoleId>id</RoleId><Arn>arn:aws:iam::123456789012:role/test-role</Arn><CreateDate>2026-01-01</CreateDate></Role></CreateRoleResult></CreateRoleResponse>");
        }).await;
        let iam = client(&server);
        let response = IamRoleWriter::new(&iam, boundary)
            .create(
                CreateRoleRequest::builder()
                    .role_name("test-role".into())
                    .assume_role_policy_document("{}".into())
                    .build(),
            )
            .await
            .unwrap();
        assert_eq!(response.create_role_result.role.role_name, "test-role");
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn put_boundary_sends_signed_iam_query_request() {
    let server = MockServer::start_async().await;
    let encoded: String = form_urlencoded::byte_serialize(BOUNDARY.as_bytes()).collect();
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .header_exists("authorization")
                .body_contains("Action=PutRolePermissionsBoundary")
                .body_contains("Version=2010-05-08")
                .body_contains("RoleName=test-role")
                .body_contains(format!("PermissionsBoundary={encoded}"));
            then.status(200)
                .body("<PutRolePermissionsBoundaryResponse/>");
        })
        .await;
    client(&server)
        .put_role_permissions_boundary("test-role", BOUNDARY)
        .await
        .unwrap();
    mock.assert_async().await;
}

fn inline_policy_page(names: &[&str], pagination: &str) -> String {
    let members = names
        .iter()
        .map(|name| format!("<member>{name}</member>"))
        .collect::<String>();
    format!("<ListRolePoliciesResponse><ListRolePoliciesResult><PolicyNames>{members}</PolicyNames>{pagination}</ListRolePoliciesResult></ListRolePoliciesResponse>")
}

fn attached_policy_page(names: &[&str], pagination: &str) -> String {
    let members = names
        .iter()
        .map(|name| format!("<member><PolicyName>{name}</PolicyName><PolicyArn>arn:aws:iam::123456789012:policy/{name}</PolicyArn></member>"))
        .collect::<String>();
    format!("<ListAttachedRolePoliciesResponse><ListAttachedRolePoliciesResult><AttachedPolicies>{members}</AttachedPolicies>{pagination}</ListAttachedRolePoliciesResult></ListAttachedRolePoliciesResponse>")
}

#[derive(Clone, Copy)]
enum PolicyListing {
    Inline,
    Attached,
}

impl PolicyListing {
    fn action(self) -> &'static str {
        match self {
            Self::Inline => "ListRolePolicies",
            Self::Attached => "ListAttachedRolePolicies",
        }
    }

    fn page(self, names: &[&str], pagination: &str) -> String {
        match self {
            Self::Inline => inline_policy_page(names, pagination),
            Self::Attached => attached_policy_page(names, pagination),
        }
    }

    async fn list(self, iam: &IamClient) -> alien_error::Result<Vec<String>, ErrorData> {
        let (names, truncated, marker) = match self {
            Self::Inline => {
                let result = iam
                    .list_role_policies("test-role")
                    .await?
                    .list_role_policies_result;
                (
                    result.policy_names.unwrap().member,
                    result.is_truncated,
                    result.marker,
                )
            }
            Self::Attached => {
                let result = iam
                    .list_attached_role_policies("test-role")
                    .await?
                    .list_attached_role_policies_result;
                let names = result
                    .attached_policies
                    .unwrap()
                    .member
                    .into_iter()
                    .map(|policy| {
                        assert_eq!(
                            policy.policy_arn,
                            format!("arn:aws:iam::123456789012:policy/{}", policy.policy_name)
                        );
                        policy.policy_name
                    })
                    .collect();
                (names, result.is_truncated, result.marker)
            }
        };
        assert_eq!(truncated, Some(false));
        assert_eq!(marker, None);
        Ok(names)
    }
}

#[tokio::test]
async fn policy_listing_collects_all_pages_including_empty_intermediate_page() {
    for listing in [PolicyListing::Inline, PolicyListing::Attached] {
        let action = listing.action();
        let server = MockServer::start_async().await;
        let first = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header_exists("authorization")
                    .body(format!(
                        "Action={action}&Version=2010-05-08&RoleName=test-role"
                    ));
                then.status(200).body(listing.page(
                    &["VolumeBackups"],
                    "<IsTruncated>true</IsTruncated><Marker>next+/=</Marker>",
                ));
            })
            .await;
        let second = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header_exists("authorization")
                    .body(format!(
                    "Action={action}&Version=2010-05-08&RoleName=test-role&Marker=next%2B%2F%3D"
                ));
                then.status(200).body(
                    listing.page(&[], "<IsTruncated>true</IsTruncated><Marker>last</Marker>"),
                );
            })
            .await;
        let last = server
            .mock_async(|when, then| {
                when.method(POST)
                    .header_exists("authorization")
                    .body(format!(
                        "Action={action}&Version=2010-05-08&RoleName=test-role&Marker=last"
                    ));
                then.status(200).body(
                    listing.page(&["BackupKey", "OldKey"], "<IsTruncated>false</IsTruncated>"),
                );
            })
            .await;
        let result = listing.list(&client(&server)).await.unwrap();
        assert_eq!(result, ["VolumeBackups", "BackupKey", "OldKey"]);
        first.assert_async().await;
        second.assert_async().await;
        last.assert_async().await;
    }
}

#[tokio::test]
async fn policy_listing_rejects_incomplete_pagination_metadata() {
    for listing in [PolicyListing::Inline, PolicyListing::Attached] {
        let action = listing.action();
        for pagination in [
            "",
            "<IsTruncated>true</IsTruncated>",
            "<IsTruncated>true</IsTruncated><Marker></Marker>",
        ] {
            let server = MockServer::start_async().await;
            let page = server
                .mock_async(|when, then| {
                    when.method(POST).body(format!(
                        "Action={action}&Version=2010-05-08&RoleName=test-role"
                    ));
                    then.status(200)
                        .body(listing.page(&["VolumeBackups"], pagination));
                })
                .await;
            let error = listing.list(&client(&server)).await.unwrap_err();
            assert!(
                matches!(error.error, Some(ErrorData::GenericError { ref message })
                if message.contains(action) && message.contains("test-role"))
            );
            page.assert_async().await;
        }
    }
}

#[tokio::test]
async fn policy_listing_rejects_marker_cycles() {
    for listing in [PolicyListing::Inline, PolicyListing::Attached] {
        let action = listing.action();
        let server = MockServer::start_async().await;
        let mut pages = Vec::new();
        for (request_marker, response_marker) in [("", "a"), ("&Marker=a", "b"), ("&Marker=b", "a")]
        {
            pages.push(
                server
                    .mock_async(|when, then| {
                        when.method(POST).body(format!(
                            "Action={action}&Version=2010-05-08&RoleName=test-role{request_marker}"
                        ));
                        then.status(200).body(listing.page(
                            &["VolumeBackups"],
                            &format!(
                                "<IsTruncated>true</IsTruncated><Marker>{response_marker}</Marker>"
                            ),
                        ));
                    })
                    .await,
            );
        }
        let error = listing.list(&client(&server)).await.unwrap_err();
        assert!(
            matches!(error.error, Some(ErrorData::GenericError { ref message })
            if message.contains("repeated a continuation marker"))
        );
        for page in pages {
            page.assert_async().await;
        }
    }
}

#[tokio::test]
async fn policy_listing_propagates_later_page_failure_without_partial_success() {
    for listing in [PolicyListing::Inline, PolicyListing::Attached] {
        let action = listing.action();
        let server = MockServer::start_async().await;
        let first = server
            .mock_async(|when, then| {
                when.method(POST).body(format!(
                    "Action={action}&Version=2010-05-08&RoleName=test-role"
                ));
                then.status(200).body(listing.page(
                    &["VolumeBackups"],
                    "<IsTruncated>true</IsTruncated><Marker>next</Marker>",
                ));
            })
            .await;
        let failed = server.mock_async(|when, then| {
            when.method(POST).body(format!("Action={action}&Version=2010-05-08&RoleName=test-role&Marker=next"));
            then.status(403).body("<ErrorResponse><Error><Code>AccessDenied</Code><Message>denied</Message></Error></ErrorResponse>");
        }).await;
        let error = listing.list(&client(&server)).await.unwrap_err();
        assert!(matches!(
            error.error,
            Some(ErrorData::RemoteAccessDenied { .. })
        ));
        first.assert_async().await;
        // The shared AWS transport may retry a failed request, but must not restart the listing.
        assert!(failed.hits_async().await > 0);
    }
}

#[tokio::test]
async fn policy_listing_accepts_empty_complete_list() {
    for listing in [PolicyListing::Inline, PolicyListing::Attached] {
        let action = listing.action();
        let server = MockServer::start_async().await;
        let page = server
            .mock_async(|when, then| {
                when.method(POST).body(format!(
                    "Action={action}&Version=2010-05-08&RoleName=test-role"
                ));
                then.status(200)
                    .body(listing.page(&[], "<IsTruncated>false</IsTruncated>"));
            })
            .await;
        let result = listing.list(&client(&server)).await.unwrap();
        assert!(result.is_empty());
        page.assert_async().await;
    }
}

#[tokio::test]
async fn adoption_audits_later_policy_pages_before_writing_boundary() {
    for aws_managed in [false, true] {
        let server = MockServer::start_async().await;
        let role = server.mock_async(|when, then| {
            when.method(POST).body_contains("Action=GetRole");
            then.status(200).body("<GetRoleResponse><GetRoleResult><Role><Path>/</Path><RoleName>test-role</RoleName><RoleId>id</RoleId><Arn>arn:aws:iam::123456789012:role/test-role</Arn><CreateDate>2026-01-01</CreateDate><Tags><member><Key>deployment</Key><Value>test</Value></member><member><Key>resource</Key><Value>worker</Value></member><member><Key>managed-by</Key><Value>runtime</Value></member></Tags></Role></GetRoleResult></GetRoleResponse>");
        }).await;
        let first = server
            .mock_async(|when, then| {
                when.method(POST)
                    .body("Action=ListAttachedRolePolicies&Version=2010-05-08&RoleName=test-role");
                then.status(200).body(attached_policy_page(
                    &[],
                    "<IsTruncated>true</IsTruncated><Marker>next</Marker>",
                ));
            })
            .await;
        let last = server.mock_async(|when, then| {
            when.method(POST).body("Action=ListAttachedRolePolicies&Version=2010-05-08&RoleName=test-role&Marker=next");
            let page = attached_policy_page(&["runtime"], "<IsTruncated>false</IsTruncated>");
            then.status(200).body(if aws_managed { page.replace("123456789012:policy/runtime", "aws:policy/AdministratorAccess") } else { page });
        }).await;
        let write = server
            .mock_async(|when, then| {
                when.method(POST)
                    .body_contains("Action=PutRolePermissionsBoundary");
                then.status(200)
                    .body("<PutRolePermissionsBoundaryResponse/>");
            })
            .await;
        let iam = client(&server);
        let result = IamRoleWriter::new(&iam, Some(BOUNDARY))
            .prepare_existing("test-role", "test", "worker")
            .await;
        if aws_managed {
            let error = result.unwrap_err();
            assert!(
                matches!(error.error, Some(ErrorData::InvalidInput { ref message, .. }) if message.contains("aws:policy/AdministratorAccess"))
            );
            write.assert_hits_async(0).await;
        } else {
            result.unwrap();
            write.assert_async().await;
        }
        role.assert_async().await;
        first.assert_async().await;
        last.assert_async().await;
    }
}
