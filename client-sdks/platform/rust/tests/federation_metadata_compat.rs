//! Additive manager metadata must remain readable by clients using other clouds.

#[test]
fn gcp_federation_metadata_extension_preserves_existing_sdk_decoding() {
    let metadata: alien_platform_api::types::ManagerManagementConfigs =
        serde_json::from_value(serde_json::json!({
            "gcp": {
                "platform": "gcp",
                "serviceAccountEmail": "manager@example-project.iam.gserviceaccount.com",
                "federationTrust": {
                    "issuer": "https://accounts.google.com",
                    "subject": "123456789012345678901"
                }
            },
            "aws": {
                "platform": "aws",
                "managingRoleArn": "arn:aws:iam::123456789012:role/manager"
            }
        }))
        .expect("an additive GCP capability must not prevent reading existing cloud metadata");
    assert_eq!(
        metadata.gcp.unwrap().service_account_email,
        "manager@example-project.iam.gserviceaccount.com"
    );
    assert!(metadata.aws.is_some());
}
