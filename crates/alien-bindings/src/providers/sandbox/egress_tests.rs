//! Each provider must reject policy changes before touching its backend.

use std::sync::Arc;

use super::{
    aws::AwsSandbox, azure::AzureSandbox, gcp_agent_platform::GcpAgentPlatformSandbox,
    kubernetes::KubernetesSandbox, local::LocalSandbox,
};
use crate::traits::{CreateSandboxRequest, Sandbox};
use alien_core::{
    bindings::{BindingValue, KubernetesSandboxBinding, LocalSandboxBinding},
    EgressPolicy, SandboxEgress,
};

#[tokio::test]
async fn every_provider_refuses_requested_policy_without_a_gateway_before_io() {
    for bound in [
        SandboxEgress::Internet,
        SandboxEgress::None,
        SandboxEgress::Allow,
        SandboxEgress::AllowDomains {
            domains: vec!["example.com".into()],
        },
    ] {
        let token = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(token.path(), "test-token").unwrap();
        let value = |s: &str| BindingValue::Value(s.to_string());
        let providers: Vec<Box<dyn Sandbox>> = vec![
            Box::new(
                AwsSandbox::new(
                    Arc::new(alien_aws_clients::aws::lambda_microvms::MockLambdaMicrovmsApi::new()),
                    "image",
                    "1",
                    vec![],
                    vec![],
                    None,
                    None,
                )
                .with_egress(Some(bound.clone())),
            ),
            Box::new(
                GcpAgentPlatformSandbox::new(
                    Arc::new(alien_gcp_clients::agent_platform::MockAgentPlatformApi::new()),
                    "engine".to_string(),
                    "template".to_string(),
                    None,
                )
                .with_egress(Some(bound.clone())),
            ),
            Box::new(AzureSandbox::new(
                Arc::new(
                    alien_azure_clients::azure::sandbox_data_plane::MockSandboxDataPlaneApi::new(),
                ),
                "group".to_string(),
                "ubuntu".to_string(),
                bound.clone(),
                None,
                "1".to_string(),
                "2Gi".to_string(),
                None,
            )),
            Box::new(
                KubernetesSandbox::new(
                    "box",
                    &KubernetesSandboxBinding {
                        default_policy: None,
                        egress: Some(bound.clone().into()),
                        namespace: value("test"),
                        runtime_class: value("gvisor"),
                        selector: value("test=box"),
                        broker_url: value("http://127.0.0.1:1"),
                        key_name: value("test"),
                        token_path: value(&token.path().display().to_string()),
                    },
                    "box",
                )
                .unwrap(),
            ),
            Box::new(
                LocalSandbox::new(
                    "box",
                    &LocalSandboxBinding {
                        default_policy: None,
                        egress: Some(bound.clone().into()),
                        manager_url: value("http://127.0.0.1:1"),
                        sandbox_key: value("box"),
                        token_path: value(&token.path().display().to_string()),
                    },
                )
                .await
                .unwrap(),
            ),
        ];
        for provider in providers {
            for (policy, code) in [
                (EgressPolicy::Internet, "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE"),
                (
                    EgressPolicy::Allow { allow: vec![] },
                    "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE",
                ),
                (
                    EgressPolicy::Allow {
                        allow: vec!["api.example.com".into()],
                    },
                    "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE",
                ),
                (
                    EgressPolicy::Allow {
                        allow: vec!["169.254.169.254".into()],
                    },
                    "SANDBOX_EGRESS_INVALID",
                ),
            ] {
                let code = if bound != SandboxEgress::Internet {
                    "SANDBOX_EGRESS_REQUIRES_INTERNET"
                } else {
                    code
                };
                let request = CreateSandboxRequest {
                    sandbox_id: Some("existing".to_string()),
                    egress: Some(policy.clone()),
                    ..Default::default()
                };
                let create = provider.create(request.clone()).await.unwrap_err();
                assert_eq!(create.code, code);
                assert!(!create.retryable);
                assert_eq!(
                    provider.get_or_create(request).await.unwrap_err().code,
                    code
                );
                assert_eq!(
                    provider
                        .update_egress("existing", policy)
                        .await
                        .unwrap_err()
                        .code,
                    code
                );
            }
        }
    }
}
