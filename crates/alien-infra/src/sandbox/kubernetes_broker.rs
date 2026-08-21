//! Claiming a warm pod and minting the capability that reaches its agent.
//!
//! This is the whole reason a broker exists. Claiming is a `PATCH` on pods, and putting that in
//! the binding would mean the customer's application holds pod-write on the namespace. That is
//! too much: `pods/exec` reaches every pod in the namespace, and the Docker socket is
//! root-equivalent on the host. A narrower credential is worth a process.
//!
//! The application gets back a pod address and a capability scoped to one session. It never gets
//! a cluster credential.

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use k8s_openapi::api::core::v1::Pod;

use crate::error::{ErrorData, Result};
use crate::sandbox::{claim_idle_pod, idle_selector};
use alien_core::sandbox_capability::{SandboxCapabilityClaims, SandboxOperationClass};
use alien_core::sandbox_capability_token;
use alien_error::{AlienError, Context};
use alien_k8s_clients::kubernetes::pods::PodApi;
use alien_k8s_clients::kubernetes::secrets::SecretsApi;

/// Port the agent serves inside a sandbox pod.
const AGENT_PORT: u16 = 8971;

/// How long a minted capability lives.
///
/// Short because a session is a unit of work someone is waiting on, and a capability that
/// outlives the turn it was minted for is a capability somebody can replay.
const CAPABILITY_LIFETIME_SECONDS: i64 = 900;

/// What the application needs to reach its session, and nothing more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedSession {
    /// Session id, which the pod now carries as a label
    pub session_id: String,
    /// `http://<pod ip>:<agent port>`
    pub endpoint: String,
    /// Bearer capability, scoped to this session and this operation class
    pub capability: String,
    /// Unix seconds after which the capability is void
    pub expires_at: i64,
}

/// Claims an idle pod for `session_id` and mints a capability addressed to it.
///
/// **The claim is won by the apiserver, not here.** `claim_idle_pod` mutates a pod in memory and
/// the write is what decides it: a conflicting write loses on `resourceVersion` and the loser
/// moves to the next candidate. Deciding locally would hand two callers the same pod.
pub async fn claim_session(
    pods: &Arc<dyn PodApi>,
    secrets: &Arc<dyn SecretsApi>,
    sandbox_id: &str,
    namespace: &str,
    session_id: &str,
    secret_name: &str,
    now_unix: i64,
) -> Result<ClaimedSession> {
    let idle = pods
        .list_pods(namespace, Some(idle_selector(sandbox_id)), None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: "failed to list idle sandbox pods".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        })?;

    let signing_key = signing_key(secrets, namespace, secret_name, sandbox_id).await?;

    for pod in idle.items {
        let Some(name) = pod.metadata.name.clone() else {
            continue;
        };

        let mut candidate = pod;
        if !claim_idle_pod(&mut candidate, session_id) {
            continue;
        }

        // A conflict here is another caller winning the same pod, not an error to report: try
        // the next one rather than failing a create that a warm pool can still satisfy.
        let Ok(claimed) = pods.update_pod(namespace, &name, &candidate).await else {
            continue;
        };

        // A pool pod is labelled idle when it is created, before the kubelet assigns an address,
        // so a claim arriving in that window can win a pod it cannot use. The label write has
        // already committed, and nothing would ever release a session the caller never received —
        // so put it back and try the next candidate, the same as losing the race on one.
        let Some(address) = claimed.status.as_ref().and_then(|status| status.pod_ip.clone()) else {
            release_claim(pods, namespace, &name, &claimed).await;
            continue;
        };

        let expires_at = now_unix + CAPABILITY_LIFETIME_SECONDS;
        let capability = sandbox_capability_token::mint(
            &SandboxCapabilityClaims {
                session_id: session_id.to_string(),
                operation: SandboxOperationClass::Execute,
                generation: 1,
                expires_at,
                key_id: secret_name.to_string(),
            },
            &signing_key,
        )
        .context(ErrorData::CloudPlatformError {
            message: "failed to mint a sandbox capability".to_string(),
            resource_id: Some(sandbox_id.to_string()),
        })?;

        return Ok(ClaimedSession {
            session_id: session_id.to_string(),
            endpoint: format!("http://{address}:{AGENT_PORT}"),
            capability,
            expires_at,
        });
    }

    // Retryable on purpose: the pool refills on the controller's health tick, so a caller that
    // waits gets a pod rather than a permanent failure.
    Err(AlienError::new(ErrorData::CloudPlatformError {
        message: format!(
            "no idle sandbox pod is available for '{sandbox_id}'; the warm pool refills on the \
             next health tick"
        ),
        resource_id: Some(sandbox_id.to_string()),
    }))
}

/// Puts a pod claimed a moment ago back in the pool.
///
/// Best effort on purpose: if this write loses or fails, the pod is one leaked slot that the
/// health tick replaces, which is strictly better than aborting a claim a later candidate could
/// have satisfied. Failing loudly here would trade a recoverable leak for a failed request.
async fn release_claim(
    pods: &Arc<dyn PodApi>,
    namespace: &str,
    name: &str,
    claimed: &Pod,
) {
    let mut restored = claimed.clone();
    if let Some(labels) = restored.metadata.labels.as_mut() {
        labels.insert(
            crate::sandbox::LABEL_POOL_STATE.to_string(),
            crate::sandbox::POOL_STATE_IDLE.to_string(),
        );
        labels.remove(crate::sandbox::LABEL_SESSION);
    }
    let _ = pods.update_pod(namespace, name, &restored).await;
}

/// Reads the sandbox's signing key out of the Secret the controller provisioned.
async fn signing_key(
    secrets: &Arc<dyn SecretsApi>,
    namespace: &str,
    secret_name: &str,
    sandbox_id: &str,
) -> Result<ed25519_compact::SecretKey> {
    let secret = secrets
        .get_secret(namespace, secret_name)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("the capability key for '{sandbox_id}' is unreadable"),
            resource_id: Some(sandbox_id.to_string()),
        })?;

    let bytes = secret
        .data
        .as_ref()
        .and_then(|data| data.get("signingKey"))
        .map(|value| value.0.clone())
        .or_else(|| {
            secret
                .string_data
                .as_ref()
                .and_then(|data| data.get("signingKey"))
                .and_then(|value| BASE64.decode(value).ok())
        })
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("the capability Secret for '{sandbox_id}' carries no key"),
                resource_id: Some(sandbox_id.to_string()),
            })
        })?;

    let pair = ed25519_compact::KeyPair::from_slice(&bytes).map_err(|error| {
        AlienError::new(ErrorData::CloudPlatformError {
            message: format!("the stored capability key is unusable: {error}"),
            resource_id: Some(sandbox_id.to_string()),
        })
    })?;

    Ok(pair.sk)
}

/// Releases a claimed session by deleting its pod.
///
/// Deleting rather than returning it to the pool: a pod that ran untrusted code cannot be handed
/// to the next session, and the warm pool refills from a clean image.
///
/// Addressed by session rather than by pod name. A claim relabels a warm pool pod instead of
/// renaming it, so the caller never learns a pod name — and taking one from the caller would let
/// any workload in the namespace delete another tenant's session, which is the pod-write this
/// broker exists to withhold. The label selector answers "is this a session of this sandbox" and
/// "which pod is it" in one query.
pub async fn release_session(
    pods: &Arc<dyn PodApi>,
    namespace: &str,
    sandbox_id: &str,
    session_id: &str,
) -> Result<()> {
    let selector = format!(
        "{}={sandbox_id},{}={session_id}",
        crate::sandbox::LABEL_SANDBOX,
        crate::sandbox::LABEL_SESSION
    );

    let claimed = pods
        .list_pods(namespace, Some(selector), None)
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("could not look up session '{session_id}'"),
            resource_id: Some(session_id.to_string()),
        })?;

    // Nothing matching is the desired end state, so release stays idempotent.
    for pod in claimed.items {
        let Some(name) = pod.metadata.name.clone() else {
            continue;
        };
        pods.delete_pod(namespace, &name)
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("could not release session '{session_id}'"),
                resource_id: Some(name),
            })?;
    }

    Ok(())
}

/// Pods belonging to a sandbox that carry a session label.
pub fn session_name(pod: &Pod) -> Option<String> {
    pod.metadata
        .labels
        .as_ref()
        .and_then(|labels| labels.get(crate::sandbox::LABEL_SESSION))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_k8s_clients::kubernetes::pods::MockPodApi;
    use alien_k8s_clients::kubernetes::secrets::MockSecretsApi;
    use k8s_openapi::api::core::v1::{PodStatus, Secret};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::List;
    use std::collections::BTreeMap;

    fn idle_pod(name: &str, ip: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                labels: Some(crate::sandbox::idle_pod_labels("sbx")),
                ..Default::default()
            },
            status: Some(PodStatus {
                pod_ip: Some(ip.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// A pool pod that exists but has not been given an address yet.
    fn idle_pod_without_address(name: &str) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                labels: Some(crate::sandbox::idle_pod_labels("sbx")),
                ..Default::default()
            },
            status: None,
            ..Default::default()
        }
    }

    fn key_secret() -> Secret {
        let pair = ed25519_compact::KeyPair::generate();
        Secret {
            data: Some(BTreeMap::from([(
                "signingKey".to_string(),
                k8s_openapi::ByteString(pair.as_ref().to_vec()),
            )])),
            ..Default::default()
        }
    }

    fn pods_returning(items: Vec<Pod>) -> MockPodApi {
        let mut pods = MockPodApi::new();
        pods.expect_list_pods()
            .returning(move |_, _, _| Ok(List { items: items.clone(), metadata: Default::default() }));
        pods
    }

    #[tokio::test]
    async fn a_claim_returns_an_address_and_a_capability_for_that_session() {
        let mut pods = pods_returning(vec![idle_pod("pool-0", "10.1.2.3")]);
        pods.expect_update_pod()
            .returning(|_, _, pod| Ok(pod.clone()));

        let mut secrets = MockSecretsApi::new();
        secrets.expect_get_secret().returning(|_, _| Ok(key_secret()));

        let claimed = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "alien-sandbox-sbx-capability",
            1_000,
        )
        .await
        .expect("an idle pod is claimable");

        assert_eq!(claimed.endpoint, "http://10.1.2.3:8971");
        assert_eq!(claimed.session_id, "s1");
        assert_eq!(claimed.expires_at, 1_000 + CAPABILITY_LIFETIME_SECONDS);
        assert!(!claimed.capability.is_empty());
    }

    /// The apiserver decides the race. A caller that lost one pod must take the next rather than
    /// failing a create the pool can still satisfy.
    #[tokio::test]
    async fn losing_the_race_on_one_pod_moves_to_the_next() {
        let mut pods = pods_returning(vec![idle_pod("pool-0", "10.1.2.3"), idle_pod("pool-1", "10.1.2.4")]);
        pods.expect_update_pod().returning(|_, name, pod| {
            if name == "pool-0" {
                Err(AlienError::new(
                    alien_client_core::ErrorData::GenericError {
                        message: "conflict".to_string(),
                    },
                ))
            } else {
                Ok(pod.clone())
            }
        });

        let mut secrets = MockSecretsApi::new();
        secrets.expect_get_secret().returning(|_, _| Ok(key_secret()));

        let claimed = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "k",
            1_000,
        )
        .await
        .expect("the second pod is claimable");

        assert_eq!(claimed.endpoint, "http://10.1.2.4:8971");
    }

    /// An empty pool is a wait, not a permanent failure: the controller refills it on its next
    /// tick, and the message says so rather than leaving a caller guessing.
    #[tokio::test]
    async fn an_empty_pool_says_it_refills() {
        let pods = pods_returning(Vec::new());
        let mut secrets = MockSecretsApi::new();
        secrets.expect_get_secret().returning(|_, _| Ok(key_secret()));

        let error = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "k",
            1_000,
        )
        .await
        .expect_err("no idle pod means no session");

        assert!(format!("{error:?}").contains("health tick"));
    }

    /// The capability the broker mints has to be the one the agent accepts. Verifying it here
    /// against the public half is what stops the two drifting into a working mint the agent
    /// refuses.
    #[tokio::test]
    async fn the_minted_capability_verifies_against_the_public_half() {
        let pair = ed25519_compact::KeyPair::generate();
        let stored = pair.as_ref().to_vec();

        let mut pods = pods_returning(vec![idle_pod("pool-0", "10.1.2.3")]);
        pods.expect_update_pod()
            .returning(|_, _, pod| Ok(pod.clone()));

        let mut secrets = MockSecretsApi::new();
        secrets.expect_get_secret().returning(move |_, _| {
            Ok(Secret {
                data: Some(BTreeMap::from([(
                    "signingKey".to_string(),
                    k8s_openapi::ByteString(stored.clone()),
                )])),
                ..Default::default()
            })
        });

        let claimed = claim_session(
            &(Arc::new(pods) as Arc<dyn PodApi>),
            &(Arc::new(secrets) as Arc<dyn SecretsApi>),
            "sbx",
            "ns",
            "s1",
            "k",
            1_000,
        )
        .await
        .expect("claim succeeds");

        sandbox_capability_token::verify(
            &claimed.capability,
            &pair.pk,
            &alien_core::sandbox_capability::SandboxSessionIdentity {
                session_id: "s1".to_string(),
                generation: 1,
            },
            SandboxOperationClass::Execute,
            1_100,
        )
        .expect("the agent must accept what the broker mints");

        sandbox_capability_token::verify(
            &claimed.capability,
            &pair.pk,
            &alien_core::sandbox_capability::SandboxSessionIdentity {
                session_id: "another-session".to_string(),
                generation: 1,
            },
            SandboxOperationClass::Execute,
            1_100,
        )
        .expect_err("a capability for one session must not reach another");
    }

    /// Release addresses a session, and the pod it deletes is the one carrying that session.
    ///
    /// A claim relabels a pool pod rather than renaming it, so the pod is still called
    /// `pool-<n>` while the caller only ever knows a session id. Deleting by the caller's string
    /// matched nothing, and the swallowed error reported success — every session leaked until the
    /// parent was torn down.
    #[tokio::test]
    async fn release_deletes_the_pod_carrying_the_session_not_one_named_after_it() {
        let mut pods = MockPodApi::new();
        pods.expect_list_pods().returning(|_, selector, _| {
            let selector = selector.expect("release must select by label");
            assert!(
                selector.contains("alien.dev/sandbox=sbx")
                    && selector.contains("alien.dev/sandbox-session=session-7"),
                "both the sandbox and the session must be in the selector: {selector}"
            );
            let mut labels = crate::sandbox::idle_pod_labels("sbx");
            labels.insert(crate::sandbox::LABEL_SESSION.to_string(), "session-7".to_string());
            Ok(List {
                items: vec![Pod {
                    metadata: ObjectMeta {
                        name: Some("pool-3".to_string()),
                        labels: Some(labels),
                        ..Default::default()
                    },
                    ..Default::default()
                }],
                ..Default::default()
            })
        });
        pods.expect_delete_pod()
            .withf(|_, name| name == "pool-3")
            .times(1)
            .returning(|_, _| Ok(()));

        let pods: Arc<dyn PodApi> = Arc::new(pods);
        release_session(&pods, "alien-sandbox-sbx", "sbx", "session-7")
            .await
            .expect("the claimed pod is released");
    }

    /// A session that matches nothing is already in the desired end state.
    #[tokio::test]
    async fn releasing_an_unknown_session_is_idempotent_and_deletes_nothing() {
        let mut pods = MockPodApi::new();
        pods.expect_list_pods()
            .returning(|_, _, _| Ok(List::default()));
        pods.expect_delete_pod().never().returning(|_, _| Ok(()));

        let pods: Arc<dyn PodApi> = Arc::new(pods);
        release_session(&pods, "alien-sandbox-sbx", "sbx", "never-claimed")
            .await
            .expect("an unknown session is not an error");
    }


    /// A pod claimed before the kubelet gave it an address is put back, not abandoned.
    ///
    /// Pool pods are labelled idle at creation, so a claim can win one that has no `pod_ip` yet.
    /// The label write has already committed and no session exists to release it, so returning an
    /// error there strands the pod in `claimed` forever and fails a request a later candidate
    /// could have served.
    #[tokio::test]
    async fn a_pod_claimed_before_it_has_an_address_is_returned_to_the_pool() {
        let mut pods = MockPodApi::new();
        pods.expect_list_pods().returning(|_, _, _| {
            Ok(List {
                items: vec![idle_pod_without_address("pool-0"), idle_pod("pool-1", "10.1.2.3")],
                ..Default::default()
            })
        });

        // pool-0 is claimed, then restored to idle; pool-1 is claimed and kept.
        pods.expect_update_pod().returning(|_, name, pod| {
            let state = pod
                .metadata
                .labels
                .as_ref()
                .and_then(|l| l.get(crate::sandbox::LABEL_POOL_STATE))
                .cloned()
                .unwrap_or_default();
            if name == "pool-0" && state == crate::sandbox::POOL_STATE_IDLE {
                return Ok(idle_pod_without_address("pool-0"));
            }
            Ok(if name == "pool-0" {
                idle_pod_without_address("pool-0")
            } else {
                idle_pod("pool-1", "10.1.2.3")
            })
        });

        let mut secrets = MockSecretsApi::new();
        secrets
            .expect_get_secret()
            .returning(|_, _| Ok(key_secret()));

        let pods: Arc<dyn PodApi> = Arc::new(pods);
        let secrets: Arc<dyn SecretsApi> = Arc::new(secrets);
        let claimed = claim_session(&pods, &secrets, "sbx", "ns", "session-1", "key", 1_000)
            .await
            .expect("the addressless pod must not fail the whole claim");

        assert_eq!(
            claimed.endpoint, "http://10.1.2.3:8971",
            "the claim must land on the pod that actually has an address"
        );
    }

}
