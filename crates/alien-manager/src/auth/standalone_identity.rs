//! Identity exchange for the standalone manager.
//!
//! - AWS and GCP workers prove their recorded role or service account and get
//!   telemetry only. The cloud keeps image pulls and command delivery native.
//! - The Kubernetes operator enrolls with a one-use code, then proves its daily
//!   key. Keys and codes live, hashed, in the manager's SQLite database.

use std::sync::Arc;

use alien_aws_clients::runtime_identity::AwsIdentityProofVerifier;
use alien_core::{ClientConfig, Platform, ResourceStatus, StackState, Worker};
use alien_error::AlienError;
use alien_gcp_clients::runtime_identity::GcpIdentityTokenVerifier;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::{
    identity_exchange::{
        IdentityEnrollmentRequest, IdentityEnrollmentResponse, IdentityExchangeRequest,
        IdentityRotationRequest, IdentityRotationResponse, RuntimeIdentityProof,
        RuntimeIdentityVerifier, VerifiedRuntimeIdentity,
    },
    runtime_capability::{RuntimeGrant, RuntimePurpose},
    Subject,
};
use crate::{
    error::ErrorData,
    stores::sqlite::{runtime_identity::DailyKeyState, SqliteRuntimeIdentityStore},
    traits::{CredentialResolver, DeploymentRecord, DeploymentStore},
};

const OPERATOR: &str = "operator";

/// Reads one deployment for the verifier, with system authority.
#[async_trait]
trait DeploymentLookup: Send + Sync {
    async fn deployment(&self, id: &str) -> Result<Option<DeploymentRecord>, AlienError>;
}

#[async_trait]
impl DeploymentLookup for Arc<dyn DeploymentStore> {
    async fn deployment(&self, id: &str) -> Result<Option<DeploymentRecord>, AlienError> {
        self.get_deployment(&Subject::system(), id).await
    }
}

pub struct StandaloneRuntimeIdentity {
    store: Arc<SqliteRuntimeIdentityStore>,
    deployments: Arc<dyn DeploymentLookup>,
    credentials: Arc<dyn CredentialResolver>,
    gcp: GcpIdentityTokenVerifier,
    clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    #[cfg(test)]
    sts_transport: Option<reqwest::Url>,
}

impl StandaloneRuntimeIdentity {
    pub fn new(
        store: Arc<SqliteRuntimeIdentityStore>,
        deployments: Arc<dyn DeploymentStore>,
        credentials: Arc<dyn CredentialResolver>,
    ) -> Result<Self, AlienError> {
        Self::with_clock(store, deployments, credentials, Arc::new(Utc::now))
    }

    pub fn with_clock(
        store: Arc<SqliteRuntimeIdentityStore>,
        deployments: Arc<dyn DeploymentStore>,
        credentials: Arc<dyn CredentialResolver>,
        clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
    ) -> Result<Self, AlienError> {
        Ok(Self {
            store,
            deployments: Arc::new(deployments),
            credentials,
            gcp: GcpIdentityTokenVerifier::with_clock(clock.clone())?,
            clock,
            #[cfg(test)]
            sts_transport: None,
        })
    }

    fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    async fn deployment(&self, id: &str) -> Result<DeploymentRecord, AlienError> {
        let record = self
            .deployments
            .deployment(id)
            .await
            .map_err(|_| unavailable())?
            .ok_or_else(refused)?;
        if matches!(record.status.as_str(), "deleting" | "deleted") {
            return Err(refused());
        }
        Ok(record)
    }

    fn grant(
        record: &DeploymentRecord,
        component_id: String,
        purpose: RuntimePurpose,
    ) -> RuntimeGrant {
        RuntimeGrant {
            workspace_id: record.workspace_id.clone(),
            project_id: record.project_id.clone(),
            deployment_id: record.id.clone(),
            component_id,
            purpose,
        }
    }

    async fn verify_aws(
        &self,
        request: &IdentityExchangeRequest,
        presigned_url: &str,
        audience: &str,
    ) -> Result<VerifiedRuntimeIdentity, AlienError> {
        let record = self.deployment(&request.deployment_id).await?;
        if request.purpose != RuntimePurpose::Telemetry || record.platform != Platform::Aws {
            return Err(refused());
        }
        let ClientConfig::Aws(config) = self
            .credentials
            .resolve(&record)
            .await
            .map_err(|_| unavailable())?
        else {
            return Err(refused());
        };
        let verifier =
            AwsIdentityProofVerifier::with_clock(config.region.clone(), self.clock.clone())?;
        #[cfg(test)]
        let verifier = match &self.sts_transport {
            Some(transport) => verifier.with_sts_transport(transport.clone()),
            None => verifier,
        };
        let principal = verifier.verify(presigned_url, audience).await?;
        if principal.account_id != config.account_id {
            return Err(refused());
        }
        // A recorded role ID names one role incarnation; a recreated role with
        // the same name has a new ID and is refused.
        let state = record.stack_state.as_ref().ok_or_else(refused)?;
        let member = aws_member(state, &principal.role_id);
        if !member {
            return Err(refused());
        }
        principal.ensure_acceptable(self.now())?;
        Ok(VerifiedRuntimeIdentity {
            grant: Self::grant(
                &record,
                format!("aws-role:{}", principal.role_id),
                RuntimePurpose::Telemetry,
            ),
            authority_deadline: principal.authority_deadline,
        })
    }

    async fn verify_gcp(
        &self,
        request: &IdentityExchangeRequest,
        token: &str,
        audience: &str,
    ) -> Result<VerifiedRuntimeIdentity, AlienError> {
        let record = self.deployment(&request.deployment_id).await?;
        if request.purpose != RuntimePurpose::Telemetry || record.platform != Platform::Gcp {
            return Err(refused());
        }
        let principal = self.gcp.verify(token, audience).await?;
        let state = record.stack_state.as_ref().ok_or_else(refused)?;
        let member = gcp_member(state, &principal.unique_id, &principal.email);
        if !member {
            return Err(refused());
        }
        principal.ensure_acceptable(self.now())?;
        Ok(VerifiedRuntimeIdentity {
            grant: Self::grant(
                &record,
                format!("gcp-service-account:{}", principal.unique_id),
                RuntimePurpose::Telemetry,
            ),
            authority_deadline: principal.authority_deadline,
        })
    }

    async fn verify_daily_key(
        &self,
        request: &IdentityExchangeRequest,
        key: &str,
    ) -> Result<VerifiedRuntimeIdentity, AlienError> {
        let state = self
            .store
            .verify_key(&request.deployment_id, key, self.now())
            .await
            .map_err(|_| unavailable())?
            .ok_or_else(refused)?;
        let record = self.deployment(&state.deployment_id).await?;
        // The enrolled operator acts for its own deployment only. Each purpose
        // still gets its own short lifetime from the issuer.
        Ok(VerifiedRuntimeIdentity {
            grant: Self::grant(&record, OPERATOR.into(), request.purpose.clone()),
            authority_deadline: state.expires_at,
        })
    }
}

/// Internal state of every running service account used by a running worker.
fn worker_service_accounts(state: &StackState) -> impl Iterator<Item = &serde_json::Value> {
    state.resources.values().filter_map(move |resource| {
        if resource.status != ResourceStatus::Running {
            return None;
        }
        let worker = resource.config.downcast_ref::<Worker>()?;
        let account = state
            .resources
            .get(&format!("{}-sa", worker.get_permissions()))?;
        if account.status != ResourceStatus::Running {
            return None;
        }
        account.internal_state.as_ref()
    })
}

/// A running worker uses the role with this immutable ID.
fn aws_member(state: &StackState, role_id: &str) -> bool {
    worker_service_accounts(state).any(|account| {
        serde_json::from_value::<alien_infra::AwsServiceAccountController>(account.clone())
            .ok()
            .and_then(|account| account.role_id)
            .as_deref()
            == Some(role_id)
    })
}

/// A running worker uses the service account with this unique ID and email.
fn gcp_member(state: &StackState, unique_id: &str, email: &str) -> bool {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RecordedAccount {
        service_account_email: Option<String>,
        service_account_unique_id: Option<String>,
    }
    worker_service_accounts(state).any(|account| {
        serde_json::from_value::<RecordedAccount>(account.clone()).is_ok_and(|account| {
            account.service_account_unique_id.as_deref() == Some(unique_id)
                && account.service_account_email.as_deref() == Some(email)
        })
    })
}

fn is_key_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn refused() -> AlienError {
    ErrorData::unauthorized("Component credential refused").into_generic()
}

fn unavailable() -> AlienError {
    AlienError::new(ErrorData::AuthenticationUnavailable).into_generic()
}

fn enrolled(state: DailyKeyState) -> IdentityEnrollmentResponse {
    IdentityEnrollmentResponse {
        deployment_id: state.deployment_id,
        component_id: OPERATOR.into(),
        expires_at: state.expires_at,
        revision: state.revision,
    }
}

#[async_trait]
impl RuntimeIdentityVerifier for StandaloneRuntimeIdentity {
    async fn enroll(
        &self,
        request: &IdentityEnrollmentRequest,
    ) -> Result<IdentityEnrollmentResponse, AlienError> {
        if !is_key_hash(&request.initial_key_hash) {
            return Err(refused());
        }
        let state = self
            .store
            .enroll(&request.code, &request.initial_key_hash, self.now())
            .await
            .map_err(|_| unavailable())?
            .ok_or_else(refused)?;
        Ok(enrolled(state))
    }

    async fn rotate(
        &self,
        request: &IdentityRotationRequest,
    ) -> Result<IdentityRotationResponse, AlienError> {
        if !is_key_hash(&request.successor_hash) {
            return Err(refused());
        }
        let state = self
            .store
            .rotate(
                &request.deployment_id,
                &request.key,
                &request.successor_hash,
                self.now(),
            )
            .await
            .map_err(|_| unavailable())?
            .ok_or_else(refused)?;
        Ok(IdentityRotationResponse {
            expires_at: state.expires_at,
            revision: state.revision,
        })
    }

    async fn verify(
        &self,
        request: &IdentityExchangeRequest,
        audience: &str,
    ) -> Result<VerifiedRuntimeIdentity, AlienError> {
        match &request.proof {
            RuntimeIdentityProof::Aws { presigned_url } => {
                self.verify_aws(request, presigned_url, audience).await
            }
            RuntimeIdentityProof::Gcp { id_token } => {
                self.verify_gcp(request, id_token, audience).await
            }
            RuntimeIdentityProof::DailyKey { key } => self.verify_daily_key(request, key).await,
            RuntimeIdentityProof::Session { .. } => Err(refused()),
        }
    }

    async fn issue_enrollment_code(
        &self,
        deployment: &DeploymentRecord,
    ) -> Result<(String, DateTime<Utc>), AlienError> {
        self.store
            .issue_enrollment_code(&deployment.id, self.now())
            .await
            .map_err(|_| unavailable())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration as StdDuration, SystemTime},
    };

    use alien_aws_clients::fake_sts::{self, Answer, FakeSts};
    use alien_core::{
        AwsClientConfig, AwsCredentials, Resource, ServiceAccount, StackResourceState, WorkerCode,
    };
    use chrono::Duration;

    use super::*;
    use crate::{ids::sha256_hash, stores::sqlite::SqliteDatabase};

    const AUDIENCE: &str = "https://manager.example";

    struct Deployments(HashMap<String, DeploymentRecord>);

    #[async_trait]
    impl DeploymentLookup for Deployments {
        async fn deployment(&self, id: &str) -> Result<Option<DeploymentRecord>, AlienError> {
            Ok(self.0.get(id).cloned())
        }
    }

    struct AwsAccount;

    #[async_trait]
    impl CredentialResolver for AwsAccount {
        async fn resolve(&self, _: &DeploymentRecord) -> Result<ClientConfig, AlienError> {
            Ok(ClientConfig::Aws(Box::new(AwsClientConfig {
                account_id: fake_sts::ACCOUNT_ID.into(),
                region: "us-east-1".into(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: "unused".into(),
                    secret_access_key: "unused".into(),
                    session_token: None,
                },
                service_overrides: None,
            })))
        }
    }

    /// A worker and its service account, both running, with a recorded role ID.
    fn aws_state(role_id: &str) -> StackState {
        let mut state = StackState::new(Platform::Aws);
        let worker = Worker::new("api".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/api@sha256:abc".to_string(),
            })
            .build();
        state.resources.insert(
            "api".into(),
            StackResourceState::builder()
                .resource_type(Worker::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(worker))
                .dependencies(Vec::new())
                .build(),
        );
        let mut role = alien_infra::AwsServiceAccountController::mock_ready("execution");
        role.role_id = Some(role_id.into());
        let role = serde_json::to_value(role).unwrap();
        state.resources.insert(
            "execution-sa".into(),
            StackResourceState::builder()
                .resource_type(ServiceAccount::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(Resource::new(
                    ServiceAccount::new("execution-sa".to_string()).build(),
                ))
                .internal_state(role)
                .dependencies(Vec::new())
                .build(),
        );
        state
    }

    fn record(id: &str, platform: Platform, state: Option<StackState>) -> DeploymentRecord {
        serde_json::from_value(serde_json::json!({
            "id": id, "workspaceId": "default", "projectId": "default", "name": id,
            "deploymentGroupId": "group", "platform": platform, "deploymentProtocolVersion": 1,
            "status": "running", "stackState": state, "environmentInfo": null,
            "runtimeMetadata": null, "currentReleaseId": null, "desiredReleaseId": null,
            "userEnvironmentVariables": null, "retryRequested": false, "lockedBy": null,
            "lockedAt": null, "createdAt": Utc::now(), "updatedAt": null, "error": null
        }))
        .unwrap()
    }

    fn signed_at() -> DateTime<Utc> {
        DateTime::from_timestamp(2_000_000_000, 0).unwrap()
    }

    /// The clock returns the queued times in order, then repeats the last one.
    fn clock(times: Vec<DateTime<Utc>>) -> Arc<dyn Fn() -> DateTime<Utc> + Send + Sync> {
        let next = Arc::new(AtomicUsize::new(0));
        Arc::new(move || times[next.fetch_add(1, Ordering::SeqCst).min(times.len() - 1)])
    }

    async fn verifier(
        records: Vec<DeploymentRecord>,
        clock: Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>,
        sts: Option<&FakeSts>,
    ) -> (StandaloneRuntimeIdentity, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let db = SqliteDatabase::new(&directory.path().join("manager.db").to_string_lossy())
            .await
            .unwrap();
        let mut verifier = StandaloneRuntimeIdentity {
            store: Arc::new(SqliteRuntimeIdentityStore::new(Arc::new(db))),
            deployments: Arc::new(Deployments(
                records.into_iter().map(|r| (r.id.clone(), r)).collect(),
            )),
            credentials: Arc::new(AwsAccount),
            gcp: GcpIdentityTokenVerifier::with_clock(clock.clone()).unwrap(),
            clock,
            sts_transport: None,
        };
        verifier.sts_transport = sts.map(|sts| sts.transport.clone());
        (verifier, directory)
    }

    fn aws_request(
        deployment: &str,
        purpose: RuntimePurpose,
        url: String,
    ) -> IdentityExchangeRequest {
        IdentityExchangeRequest {
            deployment_id: deployment.into(),
            purpose,
            proof: RuntimeIdentityProof::Aws { presigned_url: url },
        }
    }

    fn proof(sts: &FakeSts, audience: &str, expires: u64) -> String {
        sts.proof(
            audience,
            SystemTime::from(signed_at()),
            StdDuration::from_secs(expires),
        )
    }

    #[tokio::test]
    async fn recorded_aws_role_gets_telemetry_until_the_signed_hour_ends() {
        let sts = FakeSts::start("us-east-1", Answer::Identity).await;
        let (verifier, _directory) = verifier(
            vec![record(
                "dep-1",
                Platform::Aws,
                Some(aws_state(fake_sts::ROLE_ID)),
            )],
            clock(vec![signed_at() + Duration::seconds(5)]),
            Some(&sts),
        )
        .await;
        let verified = verifier
            .verify(
                &aws_request(
                    "dep-1",
                    RuntimePurpose::Telemetry,
                    proof(&sts, AUDIENCE, 300),
                ),
                AUDIENCE,
            )
            .await
            .unwrap();
        assert_eq!(verified.grant.deployment_id, "dep-1");
        assert_eq!(
            verified.grant.component_id,
            format!("aws-role:{}", fake_sts::ROLE_ID)
        );
        assert!(verified.grant.purpose == RuntimePurpose::Telemetry);
        assert_eq!(
            verified.authority_deadline,
            signed_at() + Duration::hours(1)
        );
    }

    #[tokio::test]
    async fn aws_proof_is_refused_for_another_role_deployment_audience_or_purpose() {
        let sts = FakeSts::start("us-east-1", Answer::Identity).await;
        let (verifier, _directory) = verifier(
            vec![
                record("dep-1", Platform::Aws, Some(aws_state(fake_sts::ROLE_ID))),
                record(
                    "dep-2",
                    Platform::Aws,
                    Some(aws_state("AROAOTHERROLE0000001")),
                ),
            ],
            clock(vec![signed_at() + Duration::seconds(5)]),
            Some(&sts),
        )
        .await;
        let valid = proof(&sts, AUDIENCE, 300);
        for (request, audience) in [
            // The proof's role belongs to dep-1, not dep-2.
            (
                aws_request("dep-2", RuntimePurpose::Telemetry, valid.clone()),
                AUDIENCE,
            ),
            (
                aws_request("dep-missing", RuntimePurpose::Telemetry, valid.clone()),
                AUDIENCE,
            ),
            // A proof signed for this manager is useless at another one.
            (
                aws_request("dep-1", RuntimePurpose::Telemetry, valid.clone()),
                "https://manager-two.example",
            ),
            (
                aws_request("dep-1", RuntimePurpose::OperatorSync, valid.clone()),
                AUDIENCE,
            ),
            (
                aws_request("dep-1", RuntimePurpose::RegistryPull, valid.clone()),
                AUDIENCE,
            ),
        ] {
            let error = verifier.verify(&request, audience).await.err().unwrap();
            assert!(!error.retryable);
        }
    }

    #[tokio::test]
    async fn aws_proof_that_expires_during_the_membership_check_is_refused() {
        let sts = FakeSts::start("us-east-1", Answer::Identity).await;
        // Two samples inside the proof verifier, then the one after membership.
        let (verifier, _directory) = verifier(
            vec![record(
                "dep-1",
                Platform::Aws,
                Some(aws_state(fake_sts::ROLE_ID)),
            )],
            clock(vec![
                signed_at() + Duration::seconds(30),
                signed_at() + Duration::seconds(59),
                signed_at() + Duration::seconds(60),
            ]),
            Some(&sts),
        )
        .await;
        let result = verifier
            .verify(
                &aws_request(
                    "dep-1",
                    RuntimePurpose::Telemetry,
                    proof(&sts, AUDIENCE, 60),
                ),
                AUDIENCE,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn enrolled_operator_key_works_until_rotated_and_only_for_its_deployment() {
        let now = signed_at();
        let (verifier, _directory) = verifier(
            vec![
                record("dep-1", Platform::Kubernetes, None),
                record("dep-2", Platform::Kubernetes, None),
            ],
            clock(vec![now]),
            None,
        )
        .await;
        let (code, expires_at) = verifier
            .issue_enrollment_code(&record("dep-1", Platform::Kubernetes, None))
            .await
            .unwrap();
        assert_eq!(expires_at, now + Duration::hours(1));
        let first = "ax_day_v1_first";
        let enrolled = verifier
            .enroll(&IdentityEnrollmentRequest {
                code: code.clone(),
                initial_key_hash: sha256_hash(first),
            })
            .await
            .unwrap();
        assert_eq!(enrolled.deployment_id, "dep-1");
        assert_eq!(enrolled.expires_at, now + Duration::hours(24));
        assert!(verifier
            .enroll(&IdentityEnrollmentRequest {
                code,
                initial_key_hash: sha256_hash("ax_day_v1_other"),
            })
            .await
            .is_err());

        let exchange =
            |deployment: &str, key: &str, purpose: RuntimePurpose| IdentityExchangeRequest {
                deployment_id: deployment.into(),
                purpose,
                proof: RuntimeIdentityProof::DailyKey { key: key.into() },
            };
        let verified = verifier
            .verify(
                &exchange("dep-1", first, RuntimePurpose::OperatorSync),
                AUDIENCE,
            )
            .await
            .unwrap();
        assert_eq!(verified.grant.deployment_id, "dep-1");
        assert_eq!(verified.grant.component_id, "operator");
        assert_eq!(verified.authority_deadline, now + Duration::hours(24));
        assert!(verifier
            .verify(
                &exchange("dep-2", first, RuntimePurpose::OperatorSync),
                AUDIENCE
            )
            .await
            .is_err());

        let second = "ax_day_v1_second";
        let rotated = verifier
            .rotate(&IdentityRotationRequest {
                deployment_id: "dep-1".into(),
                key: first.into(),
                successor_hash: sha256_hash(second),
            })
            .await
            .unwrap();
        assert_eq!(rotated.revision, 2);
        assert!(verifier
            .verify(
                &exchange("dep-1", first, RuntimePurpose::Telemetry),
                AUDIENCE
            )
            .await
            .is_err());
        assert!(verifier
            .verify(
                &exchange("dep-1", second, RuntimePurpose::Telemetry),
                AUDIENCE
            )
            .await
            .is_ok());
    }

    /// Pods pull images with a credential the operator writes into a Secret.
    /// It must outlive a short operator outage, and end with the daily key.
    #[tokio::test]
    async fn operator_pull_credential_lasts_until_the_daily_key_expires() {
        let now = signed_at();
        let (verifier, _directory) = verifier(
            vec![record("dep-1", Platform::Kubernetes, None)],
            clock(vec![now]),
            None,
        )
        .await;
        let (code, _) = verifier
            .issue_enrollment_code(&record("dep-1", Platform::Kubernetes, None))
            .await
            .unwrap();
        let key = "ax_day_v1_first";
        let enrolled = verifier
            .enroll(&IdentityEnrollmentRequest {
                code,
                initial_key_hash: sha256_hash(key),
            })
            .await
            .unwrap();
        let elapsed = Arc::new(AtomicUsize::new(0));
        let issuer = {
            let elapsed = elapsed.clone();
            crate::auth::runtime_capability::RuntimeCredentials::with_clock(
                AUDIENCE.into(),
                [9; 32],
                Arc::new(move || now + Duration::seconds(elapsed.load(Ordering::SeqCst) as i64)),
            )
            .unwrap()
        };
        let issue = |purpose: RuntimePurpose| async {
            let verified = verifier
                .verify(
                    &IdentityExchangeRequest {
                        deployment_id: "dep-1".into(),
                        purpose,
                        proof: RuntimeIdentityProof::DailyKey { key: key.into() },
                    },
                    AUDIENCE,
                )
                .await
                .unwrap();
            issuer
                .issue(verified.grant, verified.authority_deadline)
                .unwrap()
        };
        let pull = issue(RuntimePurpose::RegistryPull).await;
        let telemetry = issue(RuntimePurpose::Telemetry).await;
        assert_eq!(pull.expires_at, enrolled.expires_at);
        assert_eq!(telemetry.expires_at, now + Duration::hours(1));

        // The operator has been down for 70 minutes. A new pod still pulls.
        elapsed.store(70 * 60, Ordering::SeqCst);
        let subject = issuer.verify(&pull.token).unwrap();
        assert!(matches!(subject.role, crate::auth::Role::RegistryPull));
        assert!(issuer.verify(&telemetry.token).is_err());

        // Never past the daily key.
        elapsed.store(24 * 3600, Ordering::SeqCst);
        assert!(issuer.verify(&pull.token).is_err());
    }

    #[test]
    fn gcp_membership_needs_the_recorded_id_and_email_of_a_running_worker() {
        let email = "api@project.iam.gserviceaccount.com";
        let mut state = aws_state("unused");
        state.platform = Platform::Gcp;
        state
            .resources
            .get_mut("execution-sa")
            .unwrap()
            .internal_state = Some(
            serde_json::json!({"serviceAccountEmail": email, "serviceAccountUniqueId": "1234"}),
        );
        assert!(gcp_member(&state, "1234", email));
        assert!(!gcp_member(&state, "5678", email));
        assert!(!gcp_member(
            &state,
            "1234",
            "other@project.iam.gserviceaccount.com"
        ));
        state.resources.get_mut("api").unwrap().status = ResourceStatus::Deleted;
        assert!(!gcp_member(&state, "1234", email));
    }

    #[tokio::test]
    async fn malformed_key_hashes_are_refused_before_storage() {
        let (verifier, _directory) = verifier(Vec::new(), clock(vec![signed_at()]), None).await;
        for hash in ["", "ABC", &"A".repeat(64), &"g".repeat(64)] {
            assert!(verifier
                .enroll(&IdentityEnrollmentRequest {
                    code: "ax_enroll_x".into(),
                    initial_key_hash: hash.into(),
                })
                .await
                .is_err());
        }
    }
}
