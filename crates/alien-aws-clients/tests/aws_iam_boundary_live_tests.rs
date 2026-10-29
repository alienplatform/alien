//! Opt-in live test using temporary management credentials and operator-seeded roles.
//! Set AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY, AWS_SESSION_TOKEN, AWS_REGION,
//! BOUNDARY_TEST_ACCOUNT and BOUNDARY_TEST_PREFIX. Install the prefix's management
//! role and permissions-boundary policy first. Seed unbounded
//! `<prefix>-helper-{legacy,managed,setup}` roles with resource tag `helper`,
//! deployment tag `<prefix>`, managed-by `runtime` (or `setup` for setup).
//! Attach ReadOnlyAccess only to the managed fixture. The caller must verify fresh
//! names and clean up all four helper roles in a finally block, even on failure.
//! Set AWS_CA_BUNDLE to a PEM file when an additional trusted CA is required.
//! Run with `--ignored --exact live_role_writer_enforces_boundary_and_ownership`.

use std::env;

use alien_aws_clients::{
    iam::{CreateRoleRequest, CreateRoleTag, IamApi, IamClient, IamRoleWriter},
    sts::{StsApi, StsClient},
    AwsClientConfig, AwsCredentialProvider, AwsCredentials,
};
use alien_client_core::ErrorData;
use alien_core::standard_resource_tags;
use serde_json::json;

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("Set {name} explicitly for this live test"))
}

#[tokio::test]
#[ignore = "requires isolated live IAM fixtures and temporary management credentials"]
async fn live_role_writer_enforces_boundary_and_ownership() {
    let account = required("BOUNDARY_TEST_ACCOUNT");
    let prefix = required("BOUNDARY_TEST_PREFIX");
    assert!(prefix.starts_with("alien-test-") && prefix.len() <= 40);
    assert!(prefix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-'));
    assert!(account.len() == 12 && account.chars().all(|c| c.is_ascii_digit()));
    let config = AwsClientConfig {
        account_id: account.clone(),
        region: required("AWS_REGION"),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: required("AWS_ACCESS_KEY_ID"),
            secret_access_key: required("AWS_SECRET_ACCESS_KEY"),
            session_token: Some(required("AWS_SESSION_TOKEN")),
        },
        service_overrides: None,
    };
    let mut http = reqwest::Client::builder();
    if let Some(path) = env::var_os("AWS_CA_BUNDLE") {
        let pem = std::fs::read(path).expect("read configured CA bundle");
        let certificates = reqwest::Certificate::from_pem_bundle(&pem).expect("parse CA bundle");
        assert!(!certificates.is_empty(), "configured CA bundle is empty");
        for certificate in certificates {
            http = http.add_root_certificate(certificate);
        }
    }
    let http = http.build().unwrap();
    let actor = StsClient::new(http.clone(), config.clone())
        .get_caller_identity()
        .await
        .unwrap()
        .get_caller_identity_result;
    assert_eq!(actor.account.as_deref(), Some(account.as_str()));
    assert!(actor.arn.unwrap().starts_with(&format!(
        "arn:aws:sts::{account}:assumed-role/{prefix}-management/"
    )));
    let iam = IamClient::new(http, AwsCredentialProvider::from_config_sync(config));
    let boundary = format!("arn:aws:iam::{account}:policy/{prefix}-permissions-boundary");
    let writer = IamRoleWriter::new(&iam, Some(&boundary));
    let trust = json!({"Version": "2012-10-17", "Statement": [{
        "Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"},
        "Action": "sts:AssumeRole"
    }]})
    .to_string();
    let policy = json!({"Version": "2012-10-17", "Statement": [{
        "Effect": "Allow", "Action": "s3:GetObject",
        "Resource": format!("arn:aws:s3:::{prefix}-data/*")
    }]})
    .to_string();
    let created = format!("{prefix}-helper-created");
    let tags = standard_resource_tags(&prefix, "helper")
        .into_iter()
        .map(|(key, value)| CreateRoleTag { key, value })
        .collect();
    writer
        .create(
            CreateRoleRequest::builder()
                .role_name(created.clone())
                .assume_role_policy_document(trust)
                .tags(tags)
                .build(),
        )
        .await
        .unwrap();
    let role = iam.get_role(&created).await.unwrap().get_role_result.role;
    assert_eq!(
        role.permissions_boundary
            .unwrap()
            .permissions_boundary_arn
            .as_deref(),
        Some(boundary.as_str())
    );

    let legacy = format!("{prefix}-helper-legacy");
    assert!(iam
        .get_role(&legacy)
        .await
        .unwrap()
        .get_role_result
        .role
        .permissions_boundary
        .is_none());
    writer
        .prepare_existing(&legacy, &prefix, "helper")
        .await
        .unwrap();
    assert_eq!(
        iam.get_role(&legacy)
            .await
            .unwrap()
            .get_role_result
            .role
            .permissions_boundary
            .unwrap()
            .permissions_boundary_arn
            .as_deref(),
        Some(boundary.as_str())
    );
    iam.put_role_policy(&legacy, "helper", &policy)
        .await
        .unwrap();
    // Re-entry exercises the already-bounded path before a second real write.
    writer
        .prepare_existing(&legacy, &prefix, "helper")
        .await
        .unwrap();
    iam.delete_role_policy(&legacy, "helper").await.unwrap();

    for (suffix, expected_ownership_error) in [("managed", false), ("setup", true)] {
        let name = format!("{prefix}-helper-{suffix}");
        let error = writer
            .prepare_existing(&name, &prefix, "helper")
            .await
            .unwrap_err();
        if expected_ownership_error {
            assert!(matches!(
                error.error,
                Some(ErrorData::ExistingCloudResourceNotOwned { .. })
            ));
        } else {
            assert!(
                matches!(error.error, Some(ErrorData::InvalidInput { ref field_name, .. })
                if field_name.as_deref() == Some("attachedPolicies"))
            );
            assert!(error.to_string().contains("AWS-managed policy"));
        }
        assert!(iam
            .get_role(&name)
            .await
            .unwrap()
            .get_role_result
            .role
            .permissions_boundary
            .is_none());
    }
}
