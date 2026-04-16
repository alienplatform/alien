/*!
# ACM Client Integration Tests

These tests perform real AWS ACM operations including importing and deleting a certificate.

## Prerequisites

Set up `.env.test` in the workspace root with:
```
AWS_MANAGEMENT_REGION=us-east-1
AWS_MANAGEMENT_ACCESS_KEY_ID=your_access_key
AWS_MANAGEMENT_SECRET_ACCESS_KEY=your_secret_key
AWS_MANAGEMENT_ACCOUNT_ID=your_account_id
```

Optional:
```
ALIEN_TEST_ACM_DOMAIN=example.com
```
*/

use alien_aws_clients::acm::*;
use alien_aws_clients::AwsCredentialProvider;
use reqwest::Client;
use std::collections::HashSet;
use std::path::PathBuf as StdPathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tokio;
use tracing::info;

struct AcmTestContext {
    client: AcmClient,
    created_certificates: Mutex<HashSet<String>>,
    domain: String,
}

impl AsyncTestContext for AcmTestContext {
    async fn setup() -> AcmTestContext {
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
        let domain =
            std::env::var("ALIEN_TEST_ACM_DOMAIN").unwrap_or_else(|_| "example.com".to_string());

        let aws_config = alien_aws_clients::AwsClientConfig {
            account_id,
            region,
            credentials: alien_aws_clients::AwsCredentials::AccessKeys {
                access_key_id: access_key,
                secret_access_key: secret_key,
                session_token: None,
            },
            service_overrides: None,
        };

        let client = AcmClient::new(
            Client::new(),
            AwsCredentialProvider::from_config_sync(aws_config),
        );

        AcmTestContext {
            client,
            created_certificates: Mutex::new(HashSet::new()),
            domain,
        }
    }

    async fn teardown(self) {
        info!("🧹 Starting ACM test cleanup...");
        let arns: Vec<String> = self
            .created_certificates
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect();
        for arn in arns {
            let _ = self.client.delete_certificate(&arn).await;
        }
    }
}

#[test_context(AcmTestContext)]
#[tokio::test]
async fn test_import_describe_delete_certificate(ctx: &mut AcmTestContext) {
    let cert = rcgen::generate_simple_self_signed(vec![ctx.domain.clone()])
        .expect("Failed to generate self-signed certificate");
    let cert_pem = cert.cert.pem();
    let key_pem = cert.key_pair.serialize_pem();

    let response = ctx
        .client
        .import_certificate(
            ImportCertificateRequest::builder()
                .certificate(cert_pem.clone())
                .private_key(key_pem.clone())
                .build(),
        )
        .await
        .expect("Failed to import certificate");

    let arn = response.certificate_arn.clone();
    ctx.created_certificates.lock().unwrap().insert(arn.clone());

    let described = ctx
        .client
        .describe_certificate(&arn)
        .await
        .expect("Failed to describe certificate");

    let described_arn = described
        .certificate
        .as_ref()
        .and_then(|c| c.certificate_arn.clone())
        .unwrap_or_default();
    assert_eq!(described_arn, arn);

    ctx.client
        .delete_certificate(&arn)
        .await
        .expect("Failed to delete certificate");
    ctx.created_certificates.lock().unwrap().remove(&arn);
}
