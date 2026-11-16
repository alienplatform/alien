//! The operator's identity in an AWS Secrets Manager secret, for installs on
//! Amazon ECS where a task has no durable disk.
//!
//! It plugs into the same store logic as the Kubernetes Secret (`SecretBackend`
//! in `identity_store.rs`): the record is a map of text values plus a version.
//! The version is the Secrets Manager version id of `AWSCURRENT`.
//!
//! A write is two calls, so two tasks cannot overwrite each other:
//! 1. `PutSecretValue` with a fresh `ClientRequestToken` and a private staging
//!    label creates a new version that nothing reads yet.
//! 2. `UpdateSecretVersionStage` moves `AWSCURRENT` to it with
//!    `RemoveFromVersionId` set to the version that was read. Secrets Manager
//!    refuses the move when `AWSCURRENT` is no longer on that version, and the
//!    writer then re-reads.
//!
//! A crash between the two leaves an unread version with a private label that
//! holds a copy of the key. Each start removes every private label it finds
//! (`sweep_pending_labels`); a write in progress in another task at that moment
//! fails its promotion and re-reads, which the store already handles.

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_aws_clients::secrets_manager::{
    DescribeSecretRequest, GetSecretValueRequest, PutSecretValueRequest, SecretsManagerApi,
    UpdateSecretVersionStageRequest,
};
use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, Context};
use async_trait::async_trait;
use k8s_openapi::{
    api::core::v1::Secret, apimachinery::pkg::apis::meta::v1::ObjectMeta, ByteString,
};
use uuid::Uuid;

use crate::{
    error::{ErrorData, Result},
    identity_store::{PutOutcome, SecretBackend},
};

const CURRENT: &str = "AWSCURRENT";
const PENDING_PREFIX: &str = "operator-pending-";

pub(crate) struct AwsSecret {
    client: Arc<dyn SecretsManagerApi>,
    secret_id: String,
}

impl AwsSecret {
    pub(crate) fn new(client: Arc<dyn SecretsManagerApi>, secret_id: &str) -> Self {
        Self {
            client,
            secret_id: secret_id.to_string(),
        }
    }

    /// A client for the secret from the task's own credentials (the ECS task
    /// role) and region.
    pub(crate) async fn from_environment(secret_id: &str) -> Result<Self> {
        use alien_aws_clients::{
            secrets_manager::SecretsManagerClient, AwsClientConfigExt, AwsCredentialProvider,
        };
        let environment: std::collections::HashMap<String, String> = std::env::vars().collect();
        let config = alien_aws_clients::AwsClientConfig::from_env(&environment)
            .await
            .context(ErrorData::ConfigurationError {
                message: "Cannot load AWS credentials for the identity secret".into(),
            })?;
        let credentials = AwsCredentialProvider::from_config(config).await.context(
            ErrorData::ConfigurationError {
                message: "Cannot create the AWS credential provider for the identity secret".into(),
            },
        )?;
        let store = Self::new(
            Arc::new(SecretsManagerClient::new(
                reqwest::Client::new(),
                credentials,
            )),
            secret_id,
        );
        store.sweep_pending_labels().await;
        Ok(store)
    }

    /// Remove the private labels a crashed writer left behind, so their versions
    /// (and the key copy in them) are not kept. Best effort: a failure here
    /// changes nothing that is read.
    pub(crate) async fn sweep_pending_labels(&self) {
        let Ok(described) = self
            .client
            .describe_secret(DescribeSecretRequest {
                secret_id: self.secret_id.clone(),
            })
            .await
        else {
            return;
        };
        for (version, stages) in described.version_ids_to_stages.unwrap_or_default() {
            for stage in stages
                .iter()
                .filter(|stage| stage.starts_with(PENDING_PREFIX))
            {
                self.drop_label(stage, &version).await;
            }
        }
    }

    fn private_stage() -> String {
        format!("{PENDING_PREFIX}{}", Uuid::new_v4().simple())
    }

    /// Drop a private label so its version is no longer kept. Best effort.
    async fn drop_label(&self, stage: &str, version: &str) {
        let _ = self
            .client
            .update_secret_version_stage(UpdateSecretVersionStageRequest {
                secret_id: self.secret_id.clone(),
                version_stage: stage.to_string(),
                remove_from_version_id: Some(version.to_string()),
                move_to_version_id: None,
            })
            .await;
    }
}

fn record(version: &str, values: BTreeMap<String, String>) -> Secret {
    Secret {
        metadata: ObjectMeta {
            resource_version: Some(version.to_string()),
            ..Default::default()
        },
        data: Some(
            values
                .into_iter()
                .map(|(key, value)| (key, ByteString(value.into_bytes())))
                .collect(),
        ),
        ..Default::default()
    }
}

#[async_trait]
impl SecretBackend for AwsSecret {
    async fn get(&self) -> Result<Option<Secret>> {
        let response = match self
            .client
            .get_secret_value(GetSecretValueRequest {
                secret_id: self.secret_id.clone(),
                version_id: None,
                version_stage: Some(CURRENT.to_string()),
            })
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return match error.error.as_ref() {
                    Some(ClientErrorData::RemoteResourceNotFound { .. }) => Ok(None),
                    // Anything else (denied, throttled, unreachable) is not "empty".
                    _ => Err(error).context(ErrorData::ConfigurationError {
                        message: "Cannot read the identity secret".into(),
                    }),
                };
            }
        };
        let version = response.version_id.ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "The identity secret answer has no version".into(),
            })
        })?;
        let values: BTreeMap<String, String> = match response.secret_string.as_deref() {
            None | Some("") => BTreeMap::new(),
            Some(text) => serde_json::from_str(text).map_err(|_| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "The identity secret does not hold a JSON object of text values"
                        .into(),
                })
            })?,
        };
        Ok(Some(record(&version, values)))
    }

    async fn put(&self, secret: &Secret) -> Result<PutOutcome> {
        let expected = secret.metadata.resource_version.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "The identity secret was written without the version it was read at"
                    .into(),
            })
        })?;
        let values: BTreeMap<String, String> = secret
            .data
            .iter()
            .flatten()
            .map(|(key, value)| {
                String::from_utf8(value.0.clone())
                    .map(|text| (key.clone(), text))
                    .map_err(|_| {
                        AlienError::new(ErrorData::ConfigurationError {
                            message: "The identity secret holds a value that is not text".into(),
                        })
                    })
            })
            .collect::<Result<_>>()?;
        let body = serde_json::to_string(&values).map_err(|_| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "Cannot serialize the identity secret".into(),
            })
        })?;

        let stage = Self::private_stage();
        let created = match self
            .client
            .put_secret_value(PutSecretValueRequest {
                secret_id: self.secret_id.clone(),
                secret_binary: None,
                secret_string: Some(body),
                version_stages: Some(vec![stage.clone()]),
                client_request_token: Some(Uuid::new_v4().to_string()),
            })
            .await
        {
            Ok(created) => created,
            Err(error) => {
                return match error.error.as_ref() {
                    Some(ClientErrorData::RemoteResourceNotFound { .. }) => Ok(PutOutcome::Missing),
                    _ => Err(error).context(ErrorData::ConfigurationError {
                        message: "Cannot write the identity secret".into(),
                    }),
                };
            }
        };
        let new_version = created.version_id.ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "The identity secret write answer has no version".into(),
            })
        })?;

        match self
            .client
            .update_secret_version_stage(UpdateSecretVersionStageRequest {
                secret_id: self.secret_id.clone(),
                version_stage: CURRENT.to_string(),
                remove_from_version_id: Some(expected),
                move_to_version_id: Some(new_version.clone()),
            })
            .await
        {
            Ok(_) => {
                self.drop_label(&stage, &new_version).await;
                Ok(PutOutcome::Stored(record(&new_version, values)))
            }
            // The label is not on the version that was read any more: someone else
            // moved it first. Secrets Manager reports that as an invalid parameter.
            Err(error)
                if matches!(
                    error.error.as_ref(),
                    Some(ClientErrorData::InvalidInput { .. })
                        | Some(ClientErrorData::RemoteResourceConflict { .. })
                ) =>
            {
                self.drop_label(&stage, &new_version).await;
                Ok(PutOutcome::Conflict)
            }
            Err(error) => {
                self.drop_label(&stage, &new_version).await;
                Err(error).context(ErrorData::ConfigurationError {
                    message: "Cannot promote the new identity secret version".into(),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use alien_aws_clients::secrets_manager::{
        CreateSecretRequest, CreateSecretResponse, DeleteSecretRequest, DeleteSecretResponse,
        DescribeSecretRequest, DescribeSecretResponse, GetSecretValueResponse,
        PutSecretValueResponse, UpdateSecretRequest, UpdateSecretResponse,
        UpdateSecretVersionStageResponse,
    };
    use chrono::Utc;

    use super::*;
    use crate::{
        daily_identity::DailyIdentityState,
        identity_store::{IdentityStore, SecretIdentityStore},
    };

    type ClientResult<T> = std::result::Result<T, AlienError<ClientErrorData>>;

    fn client_error(data: ClientErrorData) -> AlienError<ClientErrorData> {
        AlienError::new(data)
    }

    /// Secrets Manager with the behavior the store relies on: versions carry
    /// staging labels, a label is on one version, and moving a label with
    /// `RemoveFromVersionId` fails when the label is not on that version.
    #[derive(Debug, Default)]
    struct FakeSecretsManager {
        state: Mutex<Versions>,
        unreadable: AtomicBool,
        crash_before_promote: AtomicBool,
    }

    #[derive(Debug, Default)]
    struct Versions {
        values: HashMap<String, String>,
        labels: HashMap<String, String>,
        next: u32,
        exists: bool,
    }

    impl FakeSecretsManager {
        fn with_value(value: &str) -> Arc<Self> {
            let fake = Arc::new(Self::default());
            {
                let mut state = fake.state.lock().unwrap();
                state.exists = true;
                state.next = 1;
                state.values.insert("v1".into(), value.into());
                state.labels.insert(CURRENT.into(), "v1".into());
            }
            fake
        }
        /// What an operator or a stack update does: a new current version.
        fn replace_current(&self, value: &str) {
            let mut state = self.state.lock().unwrap();
            state.next += 1;
            let id = format!("v{}", state.next);
            state.values.insert(id.clone(), value.into());
            state.labels.insert(CURRENT.into(), id);
        }
        fn private_labels(&self) -> Vec<String> {
            let state = self.state.lock().unwrap();
            state
                .labels
                .keys()
                .filter(|label| label.starts_with("operator-pending-"))
                .cloned()
                .collect()
        }
        fn current_text(&self) -> String {
            let state = self.state.lock().unwrap();
            state.values[&state.labels[CURRENT]].clone()
        }
    }

    #[async_trait]
    impl SecretsManagerApi for FakeSecretsManager {
        async fn create_secret(
            &self,
            _: CreateSecretRequest,
        ) -> ClientResult<CreateSecretResponse> {
            unimplemented!()
        }
        async fn update_secret(
            &self,
            _: UpdateSecretRequest,
        ) -> ClientResult<UpdateSecretResponse> {
            unimplemented!()
        }
        async fn delete_secret(
            &self,
            _: DeleteSecretRequest,
        ) -> ClientResult<DeleteSecretResponse> {
            unimplemented!()
        }
        async fn describe_secret(
            &self,
            _: DescribeSecretRequest,
        ) -> ClientResult<DescribeSecretResponse> {
            let state = self.state.lock().unwrap();
            let mut stages: std::collections::BTreeMap<String, Vec<String>> = Default::default();
            for (label, version) in &state.labels {
                stages
                    .entry(version.clone())
                    .or_default()
                    .push(label.clone());
            }
            Ok(
                serde_json::from_value(serde_json::json!({ "VersionIdsToStages": stages }))
                    .unwrap(),
            )
        }
        async fn get_secret_value(
            &self,
            request: GetSecretValueRequest,
        ) -> ClientResult<GetSecretValueResponse> {
            if self.unreadable.load(Ordering::Relaxed) {
                return Err(client_error(ClientErrorData::RemoteAccessDenied {
                    resource_type: "secret".into(),
                    resource_name: request.secret_id,
                }));
            }
            let state = self.state.lock().unwrap();
            let id = request.version_stage.as_deref().unwrap_or(CURRENT);
            let version = state.exists.then(|| state.labels.get(id)).flatten();
            match version {
                Some(version) => Ok(GetSecretValueResponse {
                    arn: None,
                    name: None,
                    version_id: Some(version.clone()),
                    secret_binary: None,
                    secret_string: Some(state.values[version].clone()),
                    version_stages: None,
                    created_date: None,
                }),
                None => Err(client_error(ClientErrorData::RemoteResourceNotFound {
                    resource_type: "secret".into(),
                    resource_name: request.secret_id,
                })),
            }
        }
        async fn put_secret_value(
            &self,
            request: PutSecretValueRequest,
        ) -> ClientResult<PutSecretValueResponse> {
            let mut state = self.state.lock().unwrap();
            if !state.exists {
                return Err(client_error(ClientErrorData::RemoteResourceNotFound {
                    resource_type: "secret".into(),
                    resource_name: request.secret_id,
                }));
            }
            state.next += 1;
            let id = format!("v{}", state.next);
            state
                .values
                .insert(id.clone(), request.secret_string.unwrap());
            for stage in request.version_stages.unwrap_or_default() {
                state.labels.insert(stage, id.clone());
            }
            Ok(PutSecretValueResponse {
                arn: None,
                name: None,
                version_id: Some(id),
                version_stages: None,
            })
        }
        async fn update_secret_version_stage(
            &self,
            request: UpdateSecretVersionStageRequest,
        ) -> ClientResult<UpdateSecretVersionStageResponse> {
            if self.crash_before_promote.load(Ordering::Relaxed) && request.version_stage == CURRENT
            {
                return Err(client_error(ClientErrorData::RemoteServiceUnavailable {
                    message: "the task died".into(),
                }));
            }
            let mut state = self.state.lock().unwrap();
            if let Some(from) = &request.remove_from_version_id {
                if state.labels.get(&request.version_stage) != Some(from) {
                    return Err(client_error(ClientErrorData::InvalidInput {
                        message: "the staging label is not attached to that version".into(),
                        field_name: None,
                    }));
                }
                state.labels.remove(&request.version_stage);
            }
            if let Some(to) = request.move_to_version_id {
                state.labels.insert(request.version_stage, to);
            }
            Ok(UpdateSecretVersionStageResponse {
                arn: None,
                name: None,
            })
        }
    }

    fn state(deployment: &str, current: &str) -> DailyIdentityState {
        DailyIdentityState {
            manager_url: "https://manager.example".into(),
            audience: "aud".into(),
            deployment_id: deployment.into(),
            current: current.into(),
            pending: None,
            pending_started_at: None,
            expires_at: Utc::now() + chrono::Duration::hours(20),
            enrolled_code_hash: None,
            pending_enrollment: None,
            previous: None,
        }
    }

    fn store(fake: &Arc<FakeSecretsManager>) -> SecretIdentityStore {
        SecretIdentityStore::new(
            Arc::new(AwsSecret::new(fake.clone(), "identity")),
            "identity",
        )
    }

    #[tokio::test]
    async fn identity_and_key_round_trip_and_leave_no_private_label() {
        let fake = FakeSecretsManager::with_value("{}");
        let store = store(&fake);
        assert!(store.read().await.unwrap().is_none());
        let key = store.encryption_key().await.unwrap();
        store
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        let restarted = self::store(&fake);
        assert_eq!(restarted.encryption_key().await.unwrap(), key);
        assert_eq!(restarted.read().await.unwrap().unwrap().current, "key-a");
        assert!(fake.private_labels().is_empty());
    }

    #[tokio::test]
    async fn a_stale_writer_is_refused_and_leaves_the_winner_and_no_label() {
        let fake = FakeSecretsManager::with_value("{}");
        let (a, b) = (store(&fake), store(&fake));
        a.persist_enrolled(&state("dep_1", "key-a")).await.unwrap();
        b.read().await.unwrap();
        a.write(&state("dep_1", "key-from-a")).await.unwrap();
        let error = b.write(&state("dep_1", "key-from-b")).await.unwrap_err();
        assert!(matches!(error.error, Some(ErrorData::IdentityConflict)));
        assert!(fake.current_text().contains("key-from-a"));
        assert!(fake.private_labels().is_empty());
        assert_eq!(b.read().await.unwrap().unwrap().current, "key-from-a");
        b.write(&state("dep_1", "key-from-b")).await.unwrap();
        assert!(fake.current_text().contains("key-from-b"));
    }

    #[tokio::test]
    async fn two_tasks_racing_a_rotation_leave_exactly_one_winner() {
        for _ in 0..20 {
            let fake = FakeSecretsManager::with_value("{}");
            let (a, b) = (store(&fake), store(&fake));
            a.persist_enrolled(&state("dep_1", "start")).await.unwrap();
            b.read().await.unwrap();
            a.read().await.unwrap();
            let (sa, sb) = (
                state("dep_1", "rotated-by-a"),
                state("dep_1", "rotated-by-b"),
            );
            let (ra, rb) = tokio::join!(a.write(&sa), b.write(&sb));
            assert_eq!(ra.is_ok() as u8 + rb.is_ok() as u8, 1, "{ra:?} {rb:?}");
            let text = fake.current_text();
            assert!(text.contains("rotated-by-a") ^ text.contains("rotated-by-b"));
            assert!(fake.private_labels().is_empty());
        }
    }

    #[tokio::test]
    async fn a_crash_between_put_and_promote_changes_nothing_that_is_read() {
        let fake = FakeSecretsManager::with_value("{}");
        let task = store(&fake);
        task.persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        fake.crash_before_promote.store(true, Ordering::Relaxed);
        assert!(task.write(&state("dep_1", "key-new")).await.is_err());
        fake.crash_before_promote.store(false, Ordering::Relaxed);
        // The replacement task still reads the identity that was current.
        let replacement = store(&fake);
        assert_eq!(replacement.read().await.unwrap().unwrap().current, "key-a");
        // And can rotate: the orphan version is never promoted.
        replacement.write(&state("dep_1", "key-b")).await.unwrap();
        assert_eq!(store(&fake).read().await.unwrap().unwrap().current, "key-b");
    }

    #[tokio::test]
    async fn a_start_removes_the_private_labels_a_crashed_writer_left() {
        let fake = FakeSecretsManager::with_value("{}");
        let backend = AwsSecret::new(fake.clone(), "secret");
        // The first call of a write succeeded; the process died before the second.
        fake.put_secret_value(PutSecretValueRequest {
            secret_id: "secret".into(),
            secret_binary: None,
            secret_string: Some("{\"key\":\"copy\"}".into()),
            version_stages: Some(vec![format!("{PENDING_PREFIX}dead")]),
            client_request_token: Some("0".repeat(32)),
        })
        .await
        .unwrap();
        assert_eq!(fake.private_labels().len(), 1);
        backend.sweep_pending_labels().await;
        assert!(fake.private_labels().is_empty());
        // What is read is unchanged.
        assert!(backend.get().await.unwrap().is_some());
    }

    #[tokio::test]
    async fn an_emptied_secret_gets_the_running_identity_written_back() {
        let fake = FakeSecretsManager::with_value("{}");
        let task = store(&fake);
        task.persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        let key = task.encryption_key().await.unwrap();
        // A stack update recreates the secret value as `{}`.
        fake.replace_current("{}");
        assert_eq!(task.read().await.unwrap().unwrap().current, "key-a");
        assert!(fake.current_text().contains("key-a"));
        assert!(fake.current_text().contains(&key));
    }

    #[tokio::test]
    async fn an_unreadable_secret_is_never_read_as_empty_or_replaced() {
        let fake = FakeSecretsManager::with_value("{}");
        store(&fake)
            .persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        fake.unreadable.store(true, Ordering::Relaxed);
        let fresh = store(&fake);
        assert!(fresh.read().await.is_err());
        assert!(fresh.encryption_key().await.is_err());
        assert!(fresh.write(&state("dep_2", "other")).await.is_err());
        fake.unreadable.store(false, Ordering::Relaxed);
        assert_eq!(fresh.read().await.unwrap().unwrap().deployment_id, "dep_1");
    }

    #[tokio::test]
    async fn an_enrolled_identity_is_never_replaced_by_an_unenrolled_one() {
        let fake = FakeSecretsManager::with_value("{}");
        let task = store(&fake);
        task.persist_enrolled(&state("dep_1", "key-a"))
            .await
            .unwrap();
        assert!(task.write(&state("", "candidate")).await.is_err());
        assert_eq!(task.read().await.unwrap().unwrap().deployment_id, "dep_1");
    }

    #[tokio::test]
    async fn a_missing_secret_blocks_rotation_instead_of_failing_the_operator() {
        let fake = Arc::new(FakeSecretsManager::default());
        let task = store(&fake);
        // Not found: the store keeps a key in memory and reports why it cannot save.
        assert!(task.encryption_key().await.is_ok());
        assert!(task.rotation_blocked().is_some());
    }
}
