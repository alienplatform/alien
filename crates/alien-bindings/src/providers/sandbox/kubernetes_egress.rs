//! Gateway policy delivery through the pod's authenticated agent transport.
use super::{ClaimResponse, KubernetesSandbox};
use crate::{
    error::{ErrorData, Result},
    providers::sandbox::agent_protocol::AgentTransport,
};
use alien_core::EgressPolicy;
use alien_error::{AlienError, Context, IntoAlienError};

impl KubernetesSandbox {
    pub(super) fn validate_egress(&self, requested: Option<&EgressPolicy>) -> Result<()> {
        let effective = requested.or(self.default_policy.as_ref());
        if self.egress_manager.is_some() {
            crate::providers::sandbox::egress::validate(
                effective.unwrap_or(&EgressPolicy::Internet),
                self.egress.as_ref(),
            )
        } else {
            crate::providers::sandbox::egress::require_default(effective, self.egress.as_ref())
        }
    }

    pub(super) fn egress_identity(claim: &ClaimResponse) -> Result<&str> {
        claim
            .agent_session_id
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AlienError::new(ErrorData::SandboxEgressGatewayUnavailable {
                    reason: "the Kubernetes broker did not return the immutable pod identity"
                        .into(),
                })
            })
    }

    pub(super) async fn deliver_egress(
        &self,
        id: &str,
        token: Option<&str>,
        policy: &str,
    ) -> Result<()> {
        let mut body = serde_json::json!({"policy":policy});
        if let Some(token) = token {
            body["token"] = token.into();
        }
        let response = self
            .request(id, reqwest::Method::PUT, "/v1/egress")
            .await?
            .timeout(std::time::Duration::from_secs(20))
            .json(&body)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::SandboxEgressGatewayUnavailable {
                reason: "could not deliver the policy to the Kubernetes sandbox agent".into(),
            })?;
        if response.status() != reqwest::StatusCode::NO_CONTENT {
            return Err(AlienError::new(
                ErrorData::SandboxEgressGatewayUnavailable {
                    reason: format!(
                        "Kubernetes sandbox agent refused policy delivery: {}",
                        response.status()
                    ),
                },
            ));
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;
    use crate::{
        sandbox_egress::tests::Harness,
        traits::{CreateSandboxRequest, Sandbox},
    };
    use axum::{
        http::{HeaderMap, StatusCode},
        routing::{delete, post, put},
        Json, Router,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (url, task)
    }

    async fn lifecycle(refuse_delivery: bool) {
        let gateway = Arc::new(Harness::new().await);
        let deliveries = Arc::new(Mutex::new(Vec::new()));
        let delivered = deliveries.clone();
        let stored = gateway.clone();
        let (agent, agent_task) = serve(Router::new().route(
            "/v1/egress",
            put(
                move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                    let delivered = delivered.clone();
                    let stored = stored.clone();
                    async move {
                        assert_eq!(headers["authorization"], "Bearer pod-capability");
                        assert!(
                            stored.policy("pod-uid").is_some(),
                            "gateway policy must precede delivery"
                        );
                        let record = body["policy"].as_str().unwrap();
                        let claims: serde_json::Value = serde_json::from_slice(
                            &URL_SAFE_NO_PAD
                                .decode(record.split('.').nth(1).unwrap())
                                .unwrap(),
                        )
                        .unwrap();
                        assert_eq!(claims["sub"], "pod-uid");
                        assert_eq!(claims["aud"], "prefix/box");
                        if let Some(token) = body["token"].as_str() {
                            let identity: serde_json::Value = serde_json::from_slice(
                                &URL_SAFE_NO_PAD
                                    .decode(token.split('.').nth(1).unwrap())
                                    .unwrap(),
                            )
                            .unwrap();
                            assert_eq!(identity["sub"], "pod-uid");
                        }
                        delivered.lock().unwrap().push(body);
                        if refuse_delivery {
                            StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            StatusCode::NO_CONTENT
                        }
                    }
                },
            ),
        ))
        .await;
        let released = Arc::new(Mutex::new(0));
        let releases = released.clone();
        let (broker, broker_task) = serve(Router::new()
            .route("/v1/sandbox/sessions", post(move || {
                let agent = agent.clone();
                async move { Json(serde_json::json!({"sessionId":"logical-id", "agentSessionId":"pod-uid", "endpoint":agent, "capability":"pod-capability", "expiresAt":chrono::Utc::now().timestamp()+300})) }
            }))
            .route("/v1/sandbox/box/sessions/logical-id", delete(move || {
                let releases = releases.clone();
                async move { *releases.lock().unwrap() += 1; StatusCode::NO_CONTENT }
            }))).await;
        let token = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(token.path(), "workload-service-account").unwrap();
        let sandbox = KubernetesSandbox {
            egress_manager: Some(Arc::new(gateway.reopen().await)),
            default_policy: None,
            policy_write: tokio::sync::Mutex::new(()),
            egress: Some(alien_core::SandboxEgress::Internet),
            resource_id: "box".into(),
            broker_url: broker,
            token_path: token.path().display().to_string(),
            binding_name: "box".into(),
            client: reqwest::Client::new(),
            claims: Mutex::new(BTreeMap::new()),
        };
        let created = sandbox.create(CreateSandboxRequest::default()).await;
        if refuse_delivery {
            assert_eq!(
                created.unwrap_err().code,
                "SANDBOX_EGRESS_GATEWAY_UNAVAILABLE"
            );
            assert_eq!(deliveries.lock().unwrap().len(), 1);
        } else {
            assert_eq!(created.unwrap().sandbox_id, "logical-id");
            assert_eq!(gateway.policy("pod-uid"), Some(EgressPolicy::Internet));
            assert_eq!(gateway.policy("logical-id"), None);
            let narrowed = EgressPolicy::Allow {
                allow: vec!["example.com".into()],
            };
            sandbox
                .update_egress("logical-id", narrowed.clone())
                .await
                .unwrap();
            assert_eq!(gateway.policy("pod-uid"), Some(narrowed));
            let delivered = deliveries.lock().unwrap();
            assert_eq!(delivered.len(), 2);
            assert!(delivered[0]["token"].is_string());
            assert!(
                delivered[1].get("token").is_none(),
                "updates retain the identity token"
            );
            drop(delivered);
            sandbox.terminate("logical-id").await.unwrap();
        }
        assert_eq!(*released.lock().unwrap(), 1);
        assert!(sandbox.get("logical-id").await.unwrap().is_none());
        assert!(gateway.policy("pod-uid").is_none());
        broker_task.abort();
        agent_task.abort();
    }

    #[tokio::test]
    async fn create_update_and_delete_use_the_immutable_pod_identity() {
        lifecycle(false).await;
    }

    #[tokio::test]
    async fn failed_initial_delivery_releases_the_pod_and_revokes_its_policy() {
        lifecycle(true).await;
    }
}
