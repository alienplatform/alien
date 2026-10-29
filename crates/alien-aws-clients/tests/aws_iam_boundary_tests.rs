use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use alien_aws_clients::{
    iam::{CreateRoleRequest, CreateRoleTag, IamApi, IamClient, IamRoleWriter, MockIamApi},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, ServiceOverrides,
};
use alien_client_core::ErrorData;
use alien_error::AlienError;
use httpmock::prelude::*;
use mockall::{predicate::eq, Sequence};
use serde_json::json;

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/test-permissions-boundary";

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
    let iam = MockIamApi::new();
    IamRoleWriter::new(&iam, None)
        .prepare_existing("test-role", "test", "worker")
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
async fn foreign_and_setup_owned_roles_are_never_written_or_adopted() {
    for boundary in [None, Some(BOUNDARY)] {
        for tags in [
            json!(null),
            json!({"member": []}),
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
    for result in [
        json!({"AttachedPolicies": {"member": [{"PolicyName": "AdministratorAccess", "PolicyArn": "arn:aws:iam::aws:policy/AdministratorAccess"}]}}),
        json!({"IsTruncated": true, "Marker": "next"}),
    ] {
        let mut iam = MockIamApi::new();
        expect_role(&mut iam, None);
        expect_policies(&mut iam, result);
        let error = IamRoleWriter::new(&iam, Some(BOUNDARY))
            .prepare_existing("test-role", "test", "worker")
            .await
            .unwrap_err();
        assert!(matches!(error.error, Some(ErrorData::InvalidInput { .. })));
    }
}

#[tokio::test]
async fn adoption_failure_is_returned_to_the_writer() {
    let mut iam = MockIamApi::new();
    expect_role(&mut iam, None);
    expect_policies(&mut iam, json!({}));
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

fn setup_role() -> serde_json::Value {
    json!({"Path": "/", "RoleName": "test-role", "RoleId": "id",
        "Arn": "arn:aws:iam::123456789012:role/test-role", "CreateDate": "2026-01-01",
        "Tags": {"member": [
            {"Key": "deployment", "Value": "test"},
            {"Key": "resource", "Value": "worker"},
            {"Key": "managed-by", "Value": "setup"},
            {"Key": "resource-type", "Value": "compute-cluster"},
            {"Key": "custom", "Value": "preserved"}
        ]}
    })
}

#[tokio::test]
async fn setup_handoff_recovers_after_failed_retag_and_allows_runtime_writes() {
    let role = Arc::new(Mutex::new(setup_role()));
    let mut iam = MockIamApi::new();
    let read_role = role.clone();
    iam.expect_get_role()
        .with(eq("test-role"))
        .times(4)
        .returning(move |_| {
            Ok(serde_json::from_value(
                json!({"GetRoleResult": {"Role": *read_role.lock().unwrap()}}),
            )
            .unwrap())
        });
    iam.expect_list_attached_role_policies()
        .with(eq("test-role"))
        .times(3)
        .returning(|_| {
            Ok(serde_json::from_value(json!({"ListAttachedRolePoliciesResult": {}})).unwrap())
        });
    let mut sequence = Sequence::new();
    let bounded_role = role.clone();
    iam.expect_put_role_permissions_boundary()
        .with(eq("test-role"), eq(BOUNDARY))
        .once()
        .in_sequence(&mut sequence)
        .returning(move |_, boundary| {
            bounded_role.lock().unwrap()["PermissionsBoundary"] =
                json!({"PermissionsBoundaryArn": boundary});
            Ok(())
        });
    iam.expect_tag_role()
        .once()
        .in_sequence(&mut sequence)
        .returning(|_, _| {
            Err(AlienError::new(ErrorData::InvalidInput {
                message: "tag denied".into(),
                field_name: None,
            }))
        });
    let tagged_role = role.clone();
    iam.expect_tag_role()
        .withf(|name, tags| {
            name == "test-role"
                && tags.len() == 3
                && alien_core::standard_resource_tags("test", "worker")
                    .iter()
                    .all(|(key, value)| {
                        tags.iter()
                            .any(|tag| &tag.key == key && &tag.value == value)
                    })
        })
        .once()
        .in_sequence(&mut sequence)
        .returning(move |_, tags| {
            let mut role = tagged_role.lock().unwrap();
            let existing = role["Tags"]["member"].as_array_mut().unwrap();
            for tag in tags {
                existing.retain(|old| old["Key"] != tag.key);
                existing.push(json!({"Key": tag.key, "Value": tag.value}));
            }
            Ok(())
        });
    iam.expect_put_role_policy()
        .with(eq("test-role"), eq("VolumeBackups"), eq("{}"))
        .once()
        .in_sequence(&mut sequence)
        .returning(|_, _, _| Ok(()));
    let writer = IamRoleWriter::new(&iam, Some(BOUNDARY));
    let error = writer
        .adopt_from_setup("test-role", "test", "worker", "compute-cluster")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("tag denied"));
    assert_eq!(
        role.lock().unwrap()["PermissionsBoundary"]["PermissionsBoundaryArn"],
        BOUNDARY
    );
    assert!(role.lock().unwrap()["Tags"]["member"]
        .as_array()
        .unwrap()
        .contains(&json!({"Key": "managed-by", "Value": "setup"})));
    writer
        .adopt_from_setup("test-role", "test", "worker", "compute-cluster")
        .await
        .unwrap();
    // A repeated setup handoff neither reattaches the boundary nor rewrites tags.
    writer
        .adopt_from_setup("test-role", "test", "worker", "compute-cluster")
        .await
        .unwrap();
    writer
        .prepare_existing("test-role", "test", "worker")
        .await
        .unwrap();
    iam.put_role_policy("test-role", "VolumeBackups", "{}")
        .await
        .unwrap();
    assert!(role.lock().unwrap()["Tags"]["member"]
        .as_array()
        .unwrap()
        .contains(&json!({"Key": "custom", "Value": "preserved"})));
}

#[tokio::test]
async fn setup_handoff_rejects_missing_boundary_and_foreign_ownership_before_mutation() {
    let iam = MockIamApi::new();
    let error = IamRoleWriter::new(&iam, None)
        .adopt_from_setup("test-role", "test", "worker", "compute-cluster")
        .await
        .unwrap_err();
    assert!(matches!(error.error, Some(ErrorData::InvalidInput { .. })));
    for (key, value) in [
        ("deployment", "sibling"),
        ("resource", "other"),
        ("resource-type", "other"),
        ("managed-by", "unknown"),
    ] {
        let mut role = setup_role();
        for tag in role["Tags"]["member"].as_array_mut().unwrap() {
            if tag["Key"] == key {
                tag["Value"] = json!(value);
            }
        }
        let mut iam = MockIamApi::new();
        iam.expect_get_role().once().return_once(move |_| {
            Ok(serde_json::from_value(json!({"GetRoleResult": {"Role": role}})).unwrap())
        });
        let error = IamRoleWriter::new(&iam, Some(BOUNDARY))
            .adopt_from_setup("test-role", "test", "worker", "compute-cluster")
            .await
            .unwrap_err();
        assert!(matches!(
            error.error,
            Some(ErrorData::ExistingCloudResourceNotOwned { .. })
        ));
    }
}

#[tokio::test]
async fn setup_handoff_never_retags_unsafe_or_partially_audited_roles() {
    for boundary in [None, Some(BOUNDARY), Some("other-boundary")] {
        for policies in [
            json!({"IsTruncated": true}),
            json!({"AttachedPolicies": {"member": [{"PolicyName": "DLM", "PolicyArn": "arn:aws:iam::aws:policy/service-role/AWSDataLifecycleManagerServiceRole"}]}}),
        ] {
            let mut role = setup_role();
            if let Some(boundary) = boundary {
                role["PermissionsBoundary"] = json!({"PermissionsBoundaryArn": boundary});
            }
            let mut iam = MockIamApi::new();
            iam.expect_get_role().once().return_once(move |_| {
                Ok(serde_json::from_value(json!({"GetRoleResult": {"Role": role}})).unwrap())
            });
            if boundary != Some("other-boundary") {
                expect_policies(&mut iam, policies);
            }
            let error = IamRoleWriter::new(&iam, Some(BOUNDARY))
                .adopt_from_setup("test-role", "test", "worker", "compute-cluster")
                .await
                .unwrap_err();
            assert!(matches!(error.error, Some(ErrorData::InvalidInput { .. })));
        }
    }
}

#[tokio::test]
async fn setup_handoff_does_not_retag_when_boundary_attachment_fails() {
    let mut iam = MockIamApi::new();
    iam.expect_get_role().once().returning(|_| {
        Ok(serde_json::from_value(json!({"GetRoleResult": {"Role": setup_role()}})).unwrap())
    });
    expect_policies(&mut iam, json!({}));
    iam.expect_put_role_permissions_boundary()
        .with(eq("test-role"), eq(BOUNDARY))
        .once()
        .returning(|_, _| {
            Err(AlienError::new(ErrorData::InvalidInput {
                message: "boundary denied".into(),
                field_name: None,
            }))
        });
    let error = IamRoleWriter::new(&iam, Some(BOUNDARY))
        .adopt_from_setup("test-role", "test", "worker", "compute-cluster")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("boundary denied"));
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

#[tokio::test]
async fn tag_role_sends_signed_indexed_tags_without_losing_special_characters() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .header_exists("authorization")
                .header("content-type", "application/x-www-form-urlencoded")
                .matches(|request| {
                    let params: HashMap<_, _> =
                        form_urlencoded::parse(request.body.as_deref().expect("IAM request body"))
                            .into_owned()
                            .collect();
                    params
                        == HashMap::from([
                            ("Action".into(), "TagRole".into()),
                            ("Version".into(), "2010-05-08".into()),
                            ("RoleName".into(), "test-role+backup".into()),
                            ("Tags.member.1.Key".into(), "managed-by".into()),
                            ("Tags.member.1.Value".into(), "runtime".into()),
                            ("Tags.member.2.Key".into(), "description / owner".into()),
                            (
                                "Tags.member.2.Value".into(),
                                "backup + restore = enabled".into(),
                            ),
                            ("Tags.member.3.Key".into(), "empty".into()),
                            ("Tags.member.3.Value".into(), "".into()),
                        ])
                });
            then.status(200).body("<TagRoleResponse/>");
        })
        .await;
    client(&server)
        .tag_role(
            "test-role+backup",
            &[
                CreateRoleTag {
                    key: "managed-by".into(),
                    value: "runtime".into(),
                },
                CreateRoleTag {
                    key: "description / owner".into(),
                    value: "backup + restore = enabled".into(),
                },
                CreateRoleTag {
                    key: "empty".into(),
                    value: "".into(),
                },
            ],
        )
        .await
        .expect("role tags should be updated");
    mock.assert_async().await;
}

#[tokio::test]
async fn tag_role_propagates_denied_and_missing_roles() {
    for (status, code) in [(403, "AccessDenied"), (404, "NoSuchEntity")] {
        let server = MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST).body_contains("Action=TagRole");
                then.status(status).body(format!(
                    "<ErrorResponse><Error><Type>Sender</Type><Code>{code}</Code><Message>Cannot tag role</Message></Error></ErrorResponse>"
                ));
            })
            .await;
        let error = client(&server)
            .tag_role(
                "test-role",
                &[CreateRoleTag {
                    key: "managed-by".into(),
                    value: "runtime".into(),
                }],
            )
            .await
            .expect_err("a failed tag update must not report successful handoff");
        match error.error {
            Some(ErrorData::RemoteAccessDenied { resource_name, .. }) if status == 403 => {
                assert_eq!(resource_name, "test-role");
            }
            Some(ErrorData::RemoteResourceNotFound { resource_name, .. }) if status == 404 => {
                assert_eq!(resource_name, "test-role");
            }
            other => panic!("unexpected IAM error: {other:?}"),
        }
        // The shared AWS transport may retry IAM visibility failures.
        assert!(mock.hits_async().await > 0);
    }
}
