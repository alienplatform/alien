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

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/test-permissions-boundary";

fn expect_role(iam: &mut MockIamApi, boundary: Option<&'static str>) {
    iam.expect_get_role()
        .with(eq("test-role"))
        .once()
        .returning(move |_| {
            let mut role = json!({"Path": "/", "RoleName": "test-role", "RoleId": "id",
            "Arn": "arn:aws:iam::123456789012:role/test-role", "CreateDate": "2026-01-01"});
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
        .prepare_existing("test-role")
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
        .prepare_existing("test-role")
        .await
        .unwrap();
}

#[tokio::test]
async fn already_bounded_role_needs_no_adoption() {
    let mut iam = MockIamApi::new();
    expect_role(&mut iam, Some(BOUNDARY));
    IamRoleWriter::new(&iam, Some(BOUNDARY))
        .prepare_existing("test-role")
        .await
        .unwrap();
}

#[tokio::test]
async fn different_boundary_is_not_overwritten() {
    let mut iam = MockIamApi::new();
    expect_role(&mut iam, Some("arn:aws:iam::123456789012:policy/other"));
    let error = IamRoleWriter::new(&iam, Some(BOUNDARY))
        .prepare_existing("test-role")
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
            .prepare_existing("test-role")
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
        .prepare_existing("test-role")
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
