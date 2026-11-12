//! One verifier call, from the request to the writes, for any cloud.

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_error::{AlienError, Context};
use alien_signing::{InstallId, Trust, Workload};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use tracing::info;

use crate::cloud_write::resource::{ApplyResourceWrite, ResourceFacts, ResourceWrite};
use crate::cloud_write::{own_workload, ApplyWrite, CloudWrite, Facts};
use crate::error::{ErrorData, Result};
use crate::function::Accepting;
use crate::machines::{self, DeleteMachines, RemoveMachines, RollMachines, Scale};
use crate::plan::{DeleteWorkload, OwnResource, Place, Reads, Request, SetupNetwork};
use crate::trust::{accept, find_install, Acceptance, Install, InstallStore};

/// The cloud writes one verifier makes.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Binding values the verifier reads from its own install for
    /// `workload`, by resource id.
    async fn binding_reads(
        &self,
        place: &Place,
        workload: &Workload,
    ) -> Result<BTreeMap<String, String>>;
    /// What the cloud says about the resource `own_name`, the one a workload runs as, for
    /// checking the workload's own binding. Nothing while the resource doesn't exist.
    async fn own_resource(&self, _place: &Place, _own_name: &str) -> Result<OwnResource> {
        Ok(OwnResource::default())
    }
    /// Check every field of `write`, for a resource of the signed stack, against this cloud's table.
    fn check_resource_write(
        &self,
        _facts: &ResourceFacts<'_>,
        write: &ResourceWrite,
    ) -> Result<()> {
        Err(refused(format!(
            "this cloud takes no {} write",
            write.operation()
        )))
    }
    /// Check `write` against what the cloud holds now, where the table alone can't tell.
    async fn check_resource_state(
        &self,
        _facts: &ResourceFacts<'_>,
        _write: &ResourceWrite,
    ) -> Result<()> {
        Ok(())
    }
    /// Send `write`, already checked, to the cloud unchanged.
    async fn forward_resource_write(
        &self,
        _facts: &ResourceFacts<'_>,
        write: &ResourceWrite,
    ) -> Result<Value> {
        Err(refused(format!(
            "this cloud takes no {} write",
            write.operation()
        )))
    }
    /// Check every field of `write` against this cloud's table.
    fn check_write(&self, _facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
        Err(refused(format!(
            "this cloud takes no {} write",
            write.operation()
        )))
    }
    /// Before any of `writes` goes out: refuse when what they leave alone isn't what the
    /// accepted release says.
    async fn check_state(&self, _facts: &Facts<'_>, _writes: &[CloudWrite]) -> Result<()> {
        Ok(())
    }
    /// After all of `writes` went out: refuse when the whole workload doesn't run what the
    /// accepted release and config say.
    async fn check_applied(&self, _facts: &Facts<'_>, _writes: &[CloudWrite]) -> Result<()> {
        Ok(())
    }
    /// Send `write`, already checked, to the cloud unchanged, checking `newest` right before,
    /// and read back what decides what runs. Answers what the cloud answered.
    async fn forward(&self, _facts: &Facts<'_>, write: &CloudWrite) -> Result<Value> {
        Err(refused(format!(
            "this cloud takes no {} write",
            write.operation()
        )))
    }
    /// Roll a machine group to the signed release. Every check that refuses the roll comes
    /// first; then `acceptance` records the release, and it is checked right before each write.
    async fn roll(
        &self,
        _place: &Place,
        _release: &alien_signing::ReleaseBody,
        _request: &RollMachines,
        _acceptance: &dyn Accepting,
    ) -> Result<Value> {
        Err(refused(
            "this cloud scales machines through `scale`, not `rollMachines`",
        ))
    }
    /// Resize `target` to `size` (GCP, Azure).
    async fn resize(&self, _target: &str, _zone: Option<&str>, _size: u32) -> Result<()> {
        Err(refused(
            "on AWS management sets desired capacity and AWS holds the signed bounds",
        ))
    }
    /// Delete `machines`, members of `target`, keeping at least `minimum`
    /// (GCP, Azure). Answers what was removed and the group's new size.
    async fn remove(
        &self,
        _place: &Place,
        _target: &str,
        _zone: Option<&str>,
        _machines: &[String],
        _minimum: u32,
    ) -> Result<(Vec<String>, u32)> {
        Err(refused(
            "on AWS management terminates machines and AWS holds the signed bounds",
        ))
    }
    /// Delete the function, service or app `name` with what the cloud attaches to it. Answers
    /// whether it existed.
    async fn delete_workload(&self, name: &str) -> Result<bool> {
        Err(refused(format!("this cloud deletes no workload ({name})")))
    }
    /// Take the next step in deleting machine group `target` (in `zone`, for a zonal GCP MIG)
    /// with what it boots from. Answers whether all of it is gone.
    async fn delete_machines(&self, target: &str, _zone: Option<&str>) -> Result<bool> {
        Err(refused(format!(
            "this cloud deletes no machine group ({target})"
        )))
    }
}

fn refused(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Refused {
        reason: reason.into(),
    })
}

/// A verifier: who it is, where its install keeps state, and its cloud.
pub struct Verifier {
    /// Its own resource name, `<prefix>-verifier`.
    pub own_name: String,
    /// Derives the install id for a prefix from the verifier's identity.
    pub install_id: Arc<dyn Fn(&str) -> InstallId + Send + Sync>,
    /// The install's settings, ledger and inputs.
    pub store: Arc<dyn InstallStore>,
    /// The cloud.
    pub backend: Arc<dyn Backend>,
    /// The network setup fixed for functions, if any.
    pub vpc: Option<SetupNetwork>,
    /// The network setup fixed for machines, if any.
    pub machine_network: Option<SetupNetwork>,
}

impl Verifier {
    async fn install(&self) -> Result<(Install, Place)> {
        let install = find_install(
            self.store.as_ref(),
            &self.own_name,
            self.install_id.as_ref(),
        )
        .await?;
        let place = Place {
            install: install.id.clone(),
            prefix: install.settings.prefix.clone(),
            vpc: self.vpc.clone(),
            machine_network: self.machine_network.clone(),
        };
        Ok((install, place))
    }

    /// Handle one request, given as JSON.
    pub async fn handle(&self, event: Value) -> Result<Value> {
        let request: Request = serde_json::from_value(event).map_err(|error| {
            refused(format!(
                "the request is not one the verifier accepts: {error}"
            ))
        })?;
        match request {
            Request::ApplyWrite(request) => self.write(&request).await,
            Request::ApplyResourceWrite(request) => self.resource_write(&request).await,
            Request::RollMachines(request) => self.roll(&request).await,
            Request::Scale(request) => self.scale(&request).await,
            Request::RemoveMachines(request) => self.remove(&request).await,
            Request::DeleteWorkload(request) => self.delete_workload(&request).await,
            Request::DeleteMachines(request) => self.delete_machines(&request).await,
        }
    }

    /// Take the next step in deleting a machine group of the install, `<prefix>-<cluster>-
    /// <group>`; the caller repeats the request until the answer says it is gone.
    async fn delete_machines(&self, request: &DeleteMachines) -> Result<Value> {
        let (_, place) = self.install().await?;
        let target = machines::scale_target(&place.prefix, &request.cluster, &request.group)?;
        if target == self.own_name || target == format!("{}-verifier", place.prefix) {
            return Err(refused(format!(
                "group '{}' of '{}' would be the install's verifier",
                request.group, request.cluster
            )));
        }
        let gone = self
            .backend
            .delete_machines(&target, request.zone.as_deref())
            .await?;
        info!(target = %target, gone, "deleting a machine group");
        Ok(json!({"group": target, "gone": gone}))
    }

    /// Delete a Live workload's resource. The name is the install's `<prefix>-<workload>` and
    /// never the verifier's own, which only setup's teardown removes.
    async fn delete_workload(&self, request: &DeleteWorkload) -> Result<Value> {
        let (_, place) = self.install().await?;
        let name = place.function_name(&request.workload)?;
        if name == self.own_name || name == format!("{}-verifier", place.prefix) {
            return Err(refused(format!(
                "workload '{}' would be the install's verifier",
                request.workload
            )));
        }
        let existed = self.backend.delete_workload(&name).await?;
        info!(name = %name, existed, "deleted a Live workload");
        Ok(json!({"deleted": name, "existed": existed}))
    }

    /// Apply the writes a controller built for a resource of the signed stack, after checking
    /// every one of them against the signed stack.
    async fn resource_write(&self, request: &ApplyResourceWrite) -> Result<Value> {
        let (install, place) = self.install().await?;
        let accepted = accept(
            &install,
            &request.signer_lists,
            &request.release,
            request.config.as_ref(),
            place.platform(),
            Utc::now(),
        )?;
        let stack = crate::cloud_write::resource::signed_stack(&accepted, &place)?;
        let facts = ResourceFacts {
            place: &place,
            accepted: &accepted,
            stack,
            resource_id: &request.resource,
        };
        facts.entry()?;
        if request.writes.is_empty() {
            return Err(refused(format!(
                "the request for resource '{}' carries no write",
                request.resource
            )));
        }
        for (index, write) in request.writes.iter().enumerate() {
            self.backend
                .check_resource_write(&facts, write)
                .context(ErrorData::Refused {
                    reason: format!(
                        "write {} of {} ({}) was refused, so none was applied",
                        index + 1,
                        request.writes.len(),
                        write.operation()
                    ),
                })?;
        }
        for write in &request.writes {
            self.backend.check_resource_state(&facts, write).await?;
        }
        let acceptance = Acceptance::new(self.store.as_ref(), &place.prefix, &accepted);
        acceptance.accept().await?;
        let mut answers = Vec::with_capacity(request.writes.len());
        for write in &request.writes {
            answers.push(self.backend.forward_resource_write(&facts, write).await?);
            info!(
                resource = %request.resource,
                operation = write.operation(),
                "applied the controller's checked write"
            );
        }
        Ok(json!({
            "release": accepted.release.sequence,
            "resource": request.resource,
            "operations": request.writes.iter().map(ResourceWrite::operation).collect::<Vec<_>>(),
            "answers": answers,
        }))
    }

    async fn write(&self, request: &ApplyWrite) -> Result<Value> {
        let (install, place) = self.install().await?;
        let accepted = accept(
            &install,
            &request.signer_lists,
            &request.release,
            request.config.as_ref(),
            place.platform(),
            Utc::now(),
        )?;
        let (workload, own_name) = own_workload(&place, &accepted.release.body, &request.workload)?;
        let mut reads = Reads {
            bindings: self.backend.binding_reads(&place, workload).await?,
            own: self.backend.own_resource(&place, &own_name).await?,
            ..Reads::default()
        };
        let gating = alien_core::release_statement::gating_deployer_inputs(&accepted.release.body)
            .context(ErrorData::Refused {
                reason: "the signed release's inputs can't be read".to_string(),
            })?;
        for input in workload.deployer_env.values().chain(&gating) {
            if reads.deployer_inputs.contains_key(input) {
                continue;
            }
            if let Some(value) = self.store.input(&place.prefix, input).await? {
                reads.deployer_inputs.insert(input.clone(), value);
            }
        }
        let facts = Facts {
            place: &place,
            accepted: &accepted,
            workload,
            reads: &reads,
            own_name,
        };
        if request.writes.is_empty() {
            return Err(refused(format!(
                "the request for workload '{}' carries no write",
                request.workload
            )));
        }
        // Every field of every write is checked before anything is written.
        for (index, write) in request.writes.iter().enumerate() {
            self.backend
                .check_write(&facts, write)
                .context(ErrorData::Refused {
                    reason: format!(
                        "write {} of {} ({}) was refused, so none was applied",
                        index + 1,
                        request.writes.len(),
                        write.operation()
                    ),
                })?;
        }
        self.backend.check_state(&facts, &request.writes).await?;
        // Every write passed, so the install accepts the release before the first goes out: a
        // request for an older release that arrives while these writes run is refused, and
        // can't land in the middle of them.
        let acceptance = Acceptance::new(self.store.as_ref(), &place.prefix, &accepted);
        acceptance.accept().await?;
        let mut answers = Vec::with_capacity(request.writes.len());
        for write in &request.writes {
            answers.push(self.backend.forward(&facts, write).await?);
            info!(
                function = %facts.own_name,
                operation = write.operation(),
                "applied the controller's checked write"
            );
        }
        self.backend.check_applied(&facts, &request.writes).await?;
        Ok(json!({
            "release": accepted.release.sequence,
            "function": facts.own_name,
            "operations": request.writes.iter().map(CloudWrite::operation).collect::<Vec<_>>(),
            "answers": answers,
        }))
    }

    async fn roll(&self, request: &RollMachines) -> Result<Value> {
        let (install, place) = self.install().await?;
        let accepted = accept(
            &install,
            &request.signer_lists,
            &request.release,
            request.config.as_ref(),
            place.platform(),
            Utc::now(),
        )?;
        let acceptance = Acceptance::new(self.store.as_ref(), &place.prefix, &accepted);
        self.backend
            .roll(&place, &accepted.release.body, request, &acceptance)
            .await
    }

    /// The release a scale or removal reads the signed bounds from: verified
    /// under the current signer list.
    async fn bounds_release(
        &self,
        signer_lists: &[alien_signing::SignedStatement],
        release: &alien_signing::SignedStatement,
    ) -> Result<(Place, alien_signing::Verified<alien_signing::ReleaseBody>)> {
        let (install, place) = self.install().await?;
        let now = Utc::now();
        let trust = Trust::new(
            install.settings.clone(),
            install.id.clone(),
            signer_lists,
            &install.ledger,
            now,
        )
        .context(ErrorData::Refused {
            reason: "the signer lists were refused".to_string(),
        })?;
        let release = trust
            .verify_release(release, now)
            .context(ErrorData::Refused {
                reason: "the release was refused".to_string(),
            })?;
        if release.body.platform != place.platform() {
            return Err(refused(format!(
                "the release is for platform '{}', not '{}'",
                release.body.platform,
                place.platform()
            )));
        }
        Ok((place, release))
    }

    async fn scale(&self, request: &Scale) -> Result<Value> {
        let (place, release) = self
            .bounds_release(&request.signer_lists, &request.release)
            .await?;
        let group = machines::signed_group(&release.body, &request.cluster, &request.group)?;
        let target = machines::scale_target(&place.prefix, &request.cluster, &request.group)?;
        let size = machines::clamp(request.desired, &group);
        self.backend
            .resize(&target, request.zone.as_deref(), size)
            .await?;
        info!(target = %target, desired = request.desired, size, "resized within the signed bounds");
        Ok(json!({"target": target, "size": size}))
    }

    async fn remove(&self, request: &RemoveMachines) -> Result<Value> {
        let (place, release) = self
            .bounds_release(&request.signer_lists, &request.release)
            .await?;
        let group = machines::signed_group(&release.body, &request.cluster, &request.group)?;
        let target = machines::scale_target(&place.prefix, &request.cluster, &request.group)?;
        machines::check_removal(&request.machines)?;
        let (removed, size) = self
            .backend
            .remove(
                &place,
                &target,
                request.zone.as_deref(),
                &request.machines,
                group.min_size,
            )
            .await?;
        info!(target = %target, ?removed, size, "removed machines within the signed minimum");
        Ok(json!({"group": target, "removed": removed, "size": size}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::trust::tests::MemoryStore;
    use alien_core::release_statement::{release_body, ReleaseOptions};
    use alien_core::{CapacityGroup, ComputeCluster, Platform, ResourceLifecycle, Stack};
    use alien_signing::Ledger;
    use std::sync::Mutex;

    /// A GCP backend that records each resize.
    #[derive(Default)]
    struct Resizes(Mutex<Vec<(String, Option<String>, u32)>>);

    #[async_trait]
    impl Backend for Resizes {
        async fn binding_reads(
            &self,
            _place: &Place,
            _workload: &Workload,
        ) -> Result<BTreeMap<String, String>> {
            Ok(BTreeMap::new())
        }
        async fn resize(&self, target: &str, zone: Option<&str>, size: u32) -> Result<()> {
            self.0
                .lock()
                .unwrap()
                .push((target.to_string(), zone.map(str::to_string), size));
            Ok(())
        }
        async fn remove(
            &self,
            _place: &Place,
            target: &str,
            zone: Option<&str>,
            machines: &[String],
            minimum: u32,
        ) -> Result<(Vec<String>, u32)> {
            self.0.lock().unwrap().push((
                format!("remove {target} {machines:?}"),
                zone.map(str::to_string),
                minimum,
            ));
            Ok((machines.to_vec(), 1))
        }
    }

    fn group(id: &str, max_size: u32) -> CapacityGroup {
        CapacityGroup {
            group_id: id.to_string(),
            instance_type: Some("n2-standard-4".to_string()),
            profile: None,
            min_size: 1,
            max_size,
            scale_policy: None,
            nested_virtualization: None,
            data_disk_gib_range: None,
        }
    }

    /// A GCP verifier whose install signed a cluster with a small group
    /// (`general`, at most 4) and a big one (`big`, at most 40).
    fn gcp_verifier() -> (Verifier, Arc<Resizes>, Value) {
        let list = signer_list_of(PROJECT, &ci(), Utc::now());
        let store = MemoryStore::protected(&settings_for(&list, PREFIX, CHANNEL));
        let backend = Arc::new(Resizes::default());
        let verifier = Verifier {
            own_name: format!("{PREFIX}-verifier"),
            install_id: Arc::new(|prefix| InstallId::Gcp {
                project: "shop-prod".to_string(),
                prefix: prefix.to_string(),
            }),
            store: Arc::new(store),
            backend: backend.clone(),
            vpc: None,
            machine_network: None,
        };
        let stack = Stack::new("shop".to_string())
            .add(
                ComputeCluster::new("compute".to_string())
                    .capacity_group(group("general", 4))
                    .capacity_group(group("big", 40))
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let body = release_body(
            Platform::Gcp,
            &stack,
            &ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        let release = signed_release(1, &body);
        let request = json!({"scale": {
            "cluster": "compute",
            "group": "general",
            "desired": 30,
            "release": release,
            "signerLists": [list],
        }});
        (verifier, backend, request)
    }

    fn refusal(result: Result<Value>) -> String {
        crate::describe(&result.expect_err("the request must be refused"))
    }

    #[tokio::test]
    async fn scale_resizes_the_group_the_verifier_names_within_its_signed_bounds() {
        let (verifier, backend, request) = gcp_verifier();
        let answer = verifier.handle(request).await.unwrap();
        assert_eq!(
            answer,
            json!({"target": "acme-prod-compute-general", "size": 4})
        );
        assert_eq!(
            *backend.0.lock().unwrap(),
            vec![("acme-prod-compute-general".to_string(), None, 4)]
        );
    }

    #[tokio::test]
    async fn removal_names_the_group_itself_and_passes_the_signed_minimum() {
        let (verifier, backend, scale) = gcp_verifier();
        let mut request = json!({"removeMachines": {
            "cluster": "compute", "group": "general", "zone": "us-central1-a",
            "machines": ["acme-prod-compute-general-a1b2"],
            "release": scale["scale"]["release"], "signerLists": scale["scale"]["signerLists"],
        }});
        let answer = verifier.handle(request.clone()).await.unwrap();
        assert_eq!(
            answer,
            json!({"group": "acme-prod-compute-general", "removed": ["acme-prod-compute-general-a1b2"], "size": 1})
        );
        assert_eq!(
            backend.0.lock().unwrap().clone(),
            vec![(
                "remove acme-prod-compute-general [\"acme-prod-compute-general-a1b2\"]".to_string(),
                Some("us-central1-a".to_string()),
                1
            )]
        );
        for machines in [json!([]), json!(["../x"]), json!(["a", "a"])] {
            request["removeMachines"]["machines"] = machines.clone();
            let reason = refusal(verifier.handle(request.clone()).await);
            assert!(reason.starts_with("Refused: "), "{machines}: {reason}");
        }
        assert_eq!(
            backend.0.lock().unwrap().len(),
            1,
            "refused removals reach no cloud"
        );
    }

    /// An AWS backend that checks writes with the Lambda table and records what it forwards,
    /// with the release the install's ledger recorded when it did.
    struct Forwards {
        store: Arc<MemoryStore>,
        forwarded: Mutex<Vec<(String, Option<u64>)>>,
        /// An operation whose next forward the cloud fails, once.
        fail_once: Mutex<Option<&'static str>>,
    }

    #[async_trait]
    impl Backend for Forwards {
        async fn binding_reads(
            &self,
            _place: &Place,
            _workload: &Workload,
        ) -> Result<BTreeMap<String, String>> {
            Ok(BTreeMap::new())
        }
        fn check_write(&self, facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
            crate::cloud_write::lambda::check_write(facts, write)
        }
        async fn forward(&self, _facts: &Facts<'_>, write: &CloudWrite) -> Result<Value> {
            if *self.fail_once.lock().unwrap() == Some(write.operation()) {
                *self.fail_once.lock().unwrap() = None;
                return Err(AlienError::new(ErrorData::CloudCallFailed {
                    operation: format!("lambda:{}", write.operation()),
                }));
            }
            let recorded = recorded_release(&self.store);
            self.forwarded
                .lock()
                .unwrap()
                .push((write.operation().to_string(), recorded));
            Ok(json!({"operation": write.operation()}))
        }
    }

    /// An AWS backend that checks resource writes with the real rules and records what it
    /// forwards.
    struct ResourceForwards {
        forwarded: Mutex<Vec<String>>,
        /// What the cloud says an existing mapping reads from and feeds.
        mapping: Mutex<(String, String)>,
        /// When set, the backend applies the GCP rules, and this is the IAM policy its bucket holds.
        gcp_bucket_policy: Mutex<Option<Value>>,
        /// When set, the backend applies the Azure rules.
        azure: Mutex<bool>,
    }

    #[async_trait]
    impl Backend for ResourceForwards {
        async fn binding_reads(
            &self,
            _place: &Place,
            _workload: &Workload,
        ) -> Result<BTreeMap<String, String>> {
            Ok(BTreeMap::new())
        }
        fn check_resource_write(
            &self,
            facts: &ResourceFacts<'_>,
            write: &ResourceWrite,
        ) -> Result<()> {
            if self.gcp_bucket_policy.lock().unwrap().is_some() {
                return crate::cloud_write::resource::check_gcp_write(facts, write);
            }
            if *self.azure.lock().unwrap() {
                return crate::cloud_write::resource::check_azure_write(facts, write);
            }
            crate::cloud_write::resource::check_aws_write(facts, write, REGION)
        }
        async fn check_resource_state(
            &self,
            facts: &ResourceFacts<'_>,
            write: &ResourceWrite,
        ) -> Result<()> {
            if let Some(current) = self.gcp_bucket_policy.lock().unwrap().clone() {
                return crate::cloud_write::resource::check_gcp_state(
                    facts,
                    write,
                    &current,
                    REGION,
                    Some(STORAGE_AGENT),
                );
            }
            let ResourceWrite::UpdateEventSourceMapping { uuid, .. } = write else {
                return Ok(());
            };
            let (source, function) = self.mapping.lock().unwrap().clone();
            let mut lambda = alien_aws_clients::lambda::MockLambdaApi::new();
            lambda
                .expect_get_event_source_mapping()
                .returning(move |uuid| {
                    Ok(serde_json::from_value(json!({
                        "UUID": uuid, "EventSourceArn": source, "FunctionArn": function
                    }))
                    .unwrap())
                });
            let _ = uuid;
            crate::cloud_write::resource::check_mapping_is_own(&lambda, facts, uuid).await
        }
        async fn forward_resource_write(
            &self,
            _facts: &ResourceFacts<'_>,
            write: &ResourceWrite,
        ) -> Result<Value> {
            let sent = match write {
                ResourceWrite::PutBucketPolicy { policy, .. } => policy.clone(),
                ResourceWrite::PutPublicAccessBlock { configuration, .. } => {
                    configuration.to_string()
                }
                ResourceWrite::PutBucketNotification { configuration, .. }
                | ResourceWrite::PutWorkerBucketNotification { configuration, .. } => {
                    configuration.to_string()
                }
                ResourceWrite::CreateEventSourceMapping { request }
                | ResourceWrite::UpdateEventSourceMapping { request, .. } => request.to_string(),
                ResourceWrite::CreateQueue { queue_name, .. } => queue_name.clone(),
                ResourceWrite::SetQueueAttributes { attributes, .. } => {
                    json!(attributes).to_string()
                }
                ResourceWrite::SetBucketIamPolicy { policy, .. } => policy.to_string(),
                ResourceWrite::SetPublicAccessPrevention { value, .. } => value.clone(),
                ResourceWrite::CreateTopic { body, .. }
                | ResourceWrite::CreateSubscription { body, .. }
                | ResourceWrite::CreateCommandSubscription { body, .. }
                | ResourceWrite::CreateSchedulerJob { body, .. }
                | ResourceWrite::CreateDatabaseInstance { body }
                | ResourceWrite::PatchDatabaseInstance { body, .. } => body.to_string(),
                ResourceWrite::SetTopicIamPolicy { policy, .. }
                | ResourceWrite::SetSubscriptionIamPolicy { policy, .. } => policy.to_string(),
                ResourceWrite::CreateBlobContainer { body, .. }
                | ResourceWrite::UpdateBlobContainer { body, .. }
                | ResourceWrite::PutEventSubscription { body, .. } => body.to_string(),
                ResourceWrite::CreateSandboxEngine { display_name } => display_name.clone(),
                ResourceWrite::CreateSandboxTemplate { body, .. } => body.to_string(),
                ResourceWrite::CreateSandboxDiskImage { base, .. } => base.clone(),
                ResourceWrite::CreateSandbox { body, .. } => body.to_string(),
                ResourceWrite::PutSandboxBundle { image } => image.clone(),
                ResourceWrite::CreateMicrovmImage { request }
                | ResourceWrite::UpdateMicrovmImage { request, .. } => request.to_string(),
            };
            self.forwarded.lock().unwrap().push(sent);
            Ok(json!({"operation": write.operation()}))
        }
    }

    fn storage_release(public_read: bool) -> alien_signing::SignedStatement {
        storage_release_for(alien_core::Platform::Aws, public_read)
    }

    /// A release of a stack with one storage, `assets`, which the profile `execution` may write.
    fn storage_release_for(
        platform: alien_core::Platform,
        public_read: bool,
    ) -> alien_signing::SignedStatement {
        let assets = alien_core::Storage::new("assets".to_string())
            .public_read(public_read)
            .build();
        let stack = alien_core::Stack::new("shop".to_string())
            .add(assets, alien_core::ResourceLifecycle::Frozen)
            .permission(
                "execution",
                alien_core::permissions::PermissionProfile::new()
                    .resource("assets", ["storage/data-write"]),
            )
            .build();
        let body = alien_core::release_statement::release_body(
            platform,
            &stack,
            &alien_core::release_statement::ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        signed_release(1, &body)
    }

    fn bucket_policy_request(public_read: bool, bucket: &str, statements: Vec<Value>) -> Value {
        json!({"applyResourceWrite": {
            "release": storage_release(public_read),
            "signerLists": [signer_list()],
            "resource": "assets",
            "writes": [{"putBucketPolicy": {
                "bucket": bucket,
                "policy": json!({"Version": "2012-10-17", "Statement": statements}).to_string(),
            }}],
        }})
    }

    fn resource_verifier() -> (Verifier, Arc<ResourceForwards>) {
        let store = Arc::new(MemoryStore::protected(&settings()));
        let backend = Arc::new(ResourceForwards {
            forwarded: Mutex::new(Vec::new()),
            gcp_bucket_policy: Mutex::new(None),
            azure: Mutex::new(false),
            mapping: Mutex::new((
                format!("arn:aws:sqs:{REGION}:{ACCOUNT}:{PREFIX}-jobs"),
                format!("arn:aws:lambda:{REGION}:{ACCOUNT}:function:{PREFIX}-api"),
            )),
        });
        let verifier = Verifier {
            own_name: format!("{PREFIX}-verifier"),
            install_id: Arc::new(|prefix| crate::aws::install_id(ACCOUNT, REGION, prefix)),
            store,
            backend: backend.clone(),
            vpc: None,
            machine_network: None,
        };
        (verifier, backend)
    }

    fn worker_release() -> alien_signing::SignedStatement {
        let jobs = alien_core::Queue::new("jobs".to_string()).build();
        let other = alien_core::Queue::new("other".to_string()).build();
        let api = alien_core::Worker::new("api".to_string())
            .code(alien_core::WorkerCode::Image {
                image: format!("registry.example/api@sha256:{}", "a".repeat(64)),
            })
            .permissions("execution".to_string())
            .trigger(alien_core::WorkerTrigger::Queue {
                queue: alien_core::ResourceRef::new(alien_core::Queue::RESOURCE_TYPE, "jobs"),
                batch: None,
            })
            .build();
        let stack = alien_core::Stack::new("shop".to_string())
            .add(jobs, alien_core::ResourceLifecycle::Frozen)
            .add(other, alien_core::ResourceLifecycle::Frozen)
            .add(api, alien_core::ResourceLifecycle::Live)
            .build();
        let body = alien_core::release_statement::release_body(
            alien_core::Platform::Aws,
            &stack,
            &alien_core::release_statement::ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        signed_release(1, &body)
    }

    /// A mapping reads only from a queue the signed stack triggers the worker from, and feeds
    /// only the worker's own function; the accepted request goes out unchanged.
    #[tokio::test]
    async fn event_source_mappings_are_checked_against_the_signed_stack() {
        let function = format!("{PREFIX}-api");
        let queue = |name: &str| format!("arn:aws:sqs:{REGION}:{ACCOUNT}:{PREFIX}-{name}");
        let request = |write: Value| {
            json!({"applyResourceWrite": {
                "release": worker_release(),
                "signerLists": [signer_list()],
                "resource": "api",
                "writes": [write],
            }})
        };
        let create = |source: String, target: &str| {
            request(json!({"createEventSourceMapping": {"request": {
                "EventSourceArn": source, "FunctionName": target,
                "BatchSize": 1, "Enabled": true
            }}}))
        };
        let (verifier, backend) = resource_verifier();
        verifier
            .handle(create(queue("jobs"), &function))
            .await
            .expect("the signed trigger is accepted");
        let sent: Value = serde_json::from_str(&backend.forwarded.lock().unwrap()[0]).unwrap();
        assert_eq!(sent["EventSourceArn"], queue("jobs"), "sent unchanged");
        let arn = format!("arn:aws:lambda:{REGION}:{ACCOUNT}:function:{function}");
        verifier
            .handle(create(queue("jobs"), &arn))
            .await
            .expect("the function's ARN names it too");

        for (name, write) in [
            (
                "another queue of the stack",
                create(queue("other"), &function),
            ),
            (
                "another account's queue",
                create(
                    format!("arn:aws:sqs:{REGION}:999988887777:{PREFIX}-jobs"),
                    &function,
                ),
            ),
            (
                "another function",
                create(queue("jobs"), &format!("{PREFIX}-verifier")),
            ),
            (
                "a field the verifier does not apply",
                request(json!({"createEventSourceMapping": {"request": {
                    "EventSourceArn": queue("jobs"), "FunctionName": function,
                    "SelfManagedEventSource": {}
                }}})),
            ),
            (
                "an update that moves the mapping to another function",
                request(json!({"updateEventSourceMapping": {
                    "uuid": "u-1", "request": {"FunctionName": format!("{PREFIX}-verifier")}
                }})),
            ),
        ] {
            let forwarded = backend.forwarded.lock().unwrap().len();
            let reason = refusal(verifier.handle(write).await);
            assert!(reason.starts_with("Refused: "), "{name}: {reason}");
            assert_eq!(backend.forwarded.lock().unwrap().len(), forwarded, "{name}");
        }
        let update = || {
            request(json!({"updateEventSourceMapping": {
                "uuid": "u-1", "request": {"BatchSize": 5, "FunctionName": function}
            }}))
        };
        verifier
            .handle(update())
            .await
            .expect("batch settings of the worker's own mapping are accepted");
        // A mapping that belongs to another function, or reads from another queue, is not this
        // worker's to change.
        for (name, source, target) in [
            (
                "another worker's mapping",
                queue("jobs"),
                format!("arn:aws:lambda:{REGION}:{ACCOUNT}:function:{PREFIX}-other"),
            ),
            (
                "a mapping from an unrelated queue",
                queue("other"),
                format!("arn:aws:lambda:{REGION}:{ACCOUNT}:function:{PREFIX}-api"),
            ),
        ] {
            *backend.mapping.lock().unwrap() = (source, target);
            let forwarded = backend.forwarded.lock().unwrap().len();
            let reason = refusal(verifier.handle(update()).await);
            assert!(reason.starts_with("Refused: "), "{name}: {reason}");
            assert_eq!(backend.forwarded.lock().unwrap().len(), forwarded, "{name}");
        }
    }

    /// A notification document for another bucket, or one that does not parse, is refused before
    /// the cloud is read, and nothing goes out.
    #[tokio::test]
    async fn a_notification_document_for_another_bucket_is_refused() {
        let request = |bucket: &str, configuration: Value| {
            json!({"applyResourceWrite": {
                "release": storage_release(false),
                "signerLists": [signer_list()],
                "resource": "assets",
                "writes": [{"putBucketNotification": {
                    "bucket": bucket, "configuration": configuration,
                }}],
            }})
        };
        for (bucket, configuration) in [
            ("someone-elses-assets".to_string(), json!({})),
            (
                format!("{PREFIX}-assets"),
                json!({"QueueConfiguration": "x"}),
            ),
        ] {
            let (verifier, backend) = resource_verifier();
            let reason = refusal(verifier.handle(request(&bucket, configuration)).await);
            assert!(reason.starts_with("Refused: "), "{bucket}: {reason}");
            assert!(backend.forwarded.lock().unwrap().is_empty());
        }
    }

    /// The verifier of a GCP install, whose backend applies the GCP rules to a bucket holding
    /// `current`.
    fn gcp_resource_verifier(current: &Value) -> (Verifier, Arc<ResourceForwards>) {
        let (mut verifier, backend) = resource_verifier();
        *backend.gcp_bucket_policy.lock().unwrap() = Some(current.clone());
        verifier.install_id = Arc::new(|prefix| alien_signing::InstallId::Gcp {
            project: "acme-project".to_string(),
            prefix: prefix.to_string(),
        });
        (verifier, backend)
    }

    /// A blob container's public access is checked against the signed stack, its name against
    /// the install's, and the body is sent unchanged.
    #[tokio::test]
    async fn azure_blob_container_writes_are_checked_against_the_signed_stack() {
        // A release of a given sequence is accepted once with one body, so each case gets its own
        // verifier.
        let fresh = || {
            let (mut verifier, backend) = resource_verifier();
            *backend.azure.lock().unwrap() = true;
            verifier.install_id = Arc::new(|_| alien_signing::InstallId::Azure {
                subscription: "11111111-2222-3333-4444-555555555555".to_string(),
                resource_group: PREFIX.to_string(),
            });
            (verifier, backend)
        };
        let container = format!("{PREFIX}-assets");
        let request =
            |public_read: bool, operation: &str, account: &str, name: &str, body: Value| {
                json!({"applyResourceWrite": {
                    "release": storage_release_for(alien_core::Platform::Azure, public_read),
                    "signerLists": [signer_list()],
                    "resource": "assets",
                    "writes": [{operation: {"account": account, "container": name, "body": body}}],
                }})
            };
        let private = json!({"properties": {"publicAccess": "None"}});
        let public = json!({"properties": {"publicAccess": "Blob"}});

        for (declared, operation, body) in [
            (false, "createBlobContainer", private.clone()),
            (false, "updateBlobContainer", private.clone()),
            (true, "createBlobContainer", public.clone()),
            (true, "updateBlobContainer", public.clone()),
            (false, "createBlobContainer", json!({})),
        ] {
            let (verifier, backend) = fresh();
            verifier
                .handle(request(
                    declared,
                    operation,
                    "acmestore",
                    &container,
                    body.clone(),
                ))
                .await
                .unwrap_or_else(|error| panic!("{operation} {body}: {}", crate::describe(&error)));
            assert_eq!(
                *backend.forwarded.lock().unwrap(),
                vec![body.to_string()],
                "{operation} is sent unchanged"
            );
        }

        for (what, declared, account, name, body) in [
            (
                "public on a private storage",
                false,
                "acmestore",
                container.as_str(),
                public.clone(),
            ),
            (
                "private on a public storage",
                true,
                "acmestore",
                container.as_str(),
                private.clone(),
            ),
            (
                "container access",
                false,
                "acmestore",
                container.as_str(),
                json!({"properties": {"publicAccess": "Container"}}),
            ),
            (
                "another container",
                false,
                "acmestore",
                "other",
                private.clone(),
            ),
            (
                "another field",
                false,
                "acmestore",
                container.as_str(),
                json!({"properties": {"publicAccess": "None", "defaultEncryptionScope": "x"}}),
            ),
            (
                "a top-level field",
                false,
                "acmestore",
                container.as_str(),
                json!({"properties": {}, "id": "x"}),
            ),
            (
                "an odd account",
                false,
                "../x",
                container.as_str(),
                private.clone(),
            ),
        ] {
            let (verifier, backend) = fresh();
            let result = verifier
                .handle(request(
                    declared,
                    "createBlobContainer",
                    account,
                    name,
                    body,
                ))
                .await;
            assert!(result.is_err(), "{what} must be refused");
            assert!(
                backend.forwarded.lock().unwrap().is_empty(),
                "{what}: a refusal sends nothing"
            );
        }
    }

    fn gcp_bucket_request(public_read: bool, write: Value) -> Value {
        json!({"applyResourceWrite": {
            "release": storage_release_for(alien_core::Platform::Gcp, public_read),
            "signerLists": [signer_list()],
            "resource": "assets",
            "writes": [write],
        }})
    }

    /// A bucket's IAM policy and public access prevention are checked against the signed stack and
    /// the policy the bucket holds, and sent unchanged.
    #[tokio::test]
    async fn gcp_bucket_writes_are_checked_against_the_signed_stack_and_sent_unchanged() {
        let bucket = format!("{PREFIX}-assets");
        let held = json!({"etag": "a", "bindings": [
            {"role": "roles/storage.objectAdmin", "members": ["serviceAccount:app@p.iam"]}]});
        let with_public = json!({"etag": "a", "bindings": [
            {"role": "roles/storage.objectAdmin", "members": ["serviceAccount:app@p.iam"]},
            {"role": "roles/storage.objectViewer", "members": ["allUsers"]}]});
        let with_attacker = json!({"etag": "a", "bindings": [
            {"role": "roles/storage.objectAdmin", "members": ["serviceAccount:app@p.iam", "user:attacker@x.y"]}]});
        let set_policy = |bucket: &str, policy: &Value| json!({"setBucketIamPolicy": {"bucket": bucket, "policy": policy}});
        let prevention =
            |value: &str| json!({"setPublicAccessPrevention": {"bucket": bucket, "value": value}});

        // Accepted, and sent unchanged: the held policy, the declared public read, enforcing.
        for (declared, write, sent) in [
            (false, set_policy(&bucket, &held), held.to_string()),
            (
                true,
                set_policy(&bucket, &with_public),
                with_public.to_string(),
            ),
            (false, prevention("enforced"), "enforced".to_string()),
            (true, prevention("inherited"), "inherited".to_string()),
        ] {
            let (verifier, backend) = gcp_resource_verifier(&held);
            verifier
                .handle(gcp_bucket_request(declared, write.clone()))
                .await
                .unwrap_or_else(|error| panic!("{write}: {error:?}"));
            assert_eq!(*backend.forwarded.lock().unwrap(), vec![sent], "{write}");
        }

        // The grants the signed stack derives for the bucket are accepted unchanged; the same
        // roles bound to anyone else, or to the management, verifier or setup accounts, are not.
        let stack = alien_core::Stack::new("shop".to_string())
            .add(
                alien_core::Storage::new("assets".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .permission(
                "execution",
                alien_core::permissions::PermissionProfile::new()
                    .resource("assets", ["storage/data-write"]),
            )
            .build();
        let derived = alien_permissions::gcp_resource_grants::gcp_resource_scoped_bindings(
            &stack,
            &alien_permissions::gcp_resource_grants::GcpInstall {
                prefix: PREFIX,
                project_id: "acme-project",
                project_number: None,
                region: "us-central1",
            },
            "assets",
            "storage",
            &bucket,
        )
        .unwrap();
        assert!(!derived.is_empty());
        let as_policy = |members: Option<&str>| {
            let bindings: Vec<Value> = derived
                .iter()
                .map(|grant| {
                    let mut binding = json!({"role": grant.role, "members": grant.members});
                    if let Some(condition) = &grant.condition {
                        binding["condition"] = json!({
                            "title": condition.title, "description": condition.description,
                            "expression": condition.expression});
                    }
                    if let Some(member) = members {
                        binding["members"] = json!([member]);
                    }
                    binding
                })
                .collect();
            json!({"etag": "a", "version": 3, "bindings": bindings})
        };
        let (verifier, backend) = gcp_resource_verifier(&json!({}));
        verifier
            .handle(gcp_bucket_request(
                false,
                set_policy(&bucket, &as_policy(None)),
            ))
            .await
            .expect("the derived grants are accepted");
        assert_eq!(
            *backend.forwarded.lock().unwrap(),
            vec![as_policy(None).to_string()],
            "sent unchanged"
        );
        for member in [
            "serviceAccount:acme-prod-management@acme-project.iam.gserviceaccount.com",
            "serviceAccount:acme-prod-verifier@acme-project.iam.gserviceaccount.com",
            "user:attacker@example.com",
        ] {
            let (verifier, backend) = gcp_resource_verifier(&json!({}));
            let reason = refusal(
                verifier
                    .handle(gcp_bucket_request(
                        false,
                        set_policy(&bucket, &as_policy(Some(member))),
                    ))
                    .await,
            );
            assert!(reason.contains(member), "{member}: {reason}");
            assert!(
                backend.forwarded.lock().unwrap().is_empty(),
                "{member}: sent"
            );
        }

        // Refused, and nothing is sent.
        for (name, declared, write) in [
            (
                "public read the stack does not declare",
                false,
                set_policy(&bucket, &with_public),
            ),
            (
                "a member the bucket does not hold",
                true,
                set_policy(&bucket, &with_attacker),
            ),
            (
                "another bucket",
                false,
                set_policy("someone-elses-bucket", &held),
            ),
            (
                "inherited prevention not declared",
                false,
                prevention("inherited"),
            ),
        ] {
            let (verifier, backend) = gcp_resource_verifier(&held);
            refusal(verifier.handle(gcp_bucket_request(declared, write)).await);
            assert!(backend.forwarded.lock().unwrap().is_empty(), "{name}: sent");
        }
    }

    const STORAGE_AGENT: &str = "service-1@gs-project-accounts.iam.gserviceaccount.com";

    fn gcp_queue_release(notified_by_storage: bool) -> alien_signing::SignedStatement {
        let mut assets = alien_core::Storage::new("assets".to_string()).build();
        if notified_by_storage {
            assets.notifications = vec![alien_core::StorageNotification::builder()
                .queue(alien_core::ResourceRef::new(
                    alien_core::Queue::RESOURCE_TYPE,
                    "jobs".to_string(),
                ))
                .build()];
        }
        let stack = alien_core::Stack::new("shop".to_string())
            .add(assets, alien_core::ResourceLifecycle::Frozen)
            .add(
                alien_core::Queue::new("jobs".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .permission(
                "execution",
                alien_core::permissions::PermissionProfile::new()
                    .resource("jobs", ["queue/data-read"]),
            )
            .build();
        let body = alien_core::release_statement::release_body(
            alien_core::Platform::Gcp,
            &stack,
            &alien_core::release_statement::ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        signed_release(1, &body)
    }

    /// A queue's topic and subscription are created as the controller builds them, and their IAM
    /// policies may grant only what the signed stack derives, plus the Cloud Storage service agent
    /// on a topic a storage of the stack notifies.
    #[tokio::test]
    async fn gcp_queue_writes_are_checked_against_the_signed_stack_and_sent_unchanged() {
        let topic = format!("{PREFIX}-jobs");
        let subscription = format!("{topic}-sub");
        let request = |notified: bool, write: Value| {
            json!({"applyResourceWrite": {
                "release": gcp_queue_release(notified),
                "signerLists": [signer_list()],
                "resource": "jobs",
                "writes": [write],
            }})
        };
        let agent_policy = json!({"bindings": [{
            "role": "roles/pubsub.publisher",
            "members": [format!("serviceAccount:{STORAGE_AGENT}")]}]});
        let attacker_policy = json!({"bindings": [{
            "role": "roles/pubsub.publisher", "members": ["user:attacker@example.com"]}]});
        let own_topic = format!("projects/acme-project/topics/{topic}");

        for (name, notified, write, sent) in [
            (
                "topic",
                false,
                json!({"createTopic": {"topic": topic, "body": {}}}),
                "{}".to_string(),
            ),
            (
                "subscription",
                false,
                json!({"createSubscription": {"subscription": subscription, "body": {"topic": own_topic}}}),
                json!({"topic": own_topic}).to_string(),
            ),
            (
                "storage agent publishing to a notified topic",
                true,
                json!({"setTopicIamPolicy": {"topic": topic, "policy": agent_policy}}),
                agent_policy.to_string(),
            ),
        ] {
            let (verifier, backend) = gcp_resource_verifier(&json!({}));
            verifier
                .handle(request(notified, write))
                .await
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!(*backend.forwarded.lock().unwrap(), vec![sent], "{name}");
        }

        for (name, notified, write) in [
            (
                "a topic with a label",
                false,
                json!({"createTopic": {"topic": topic, "body": {"labels": {"a": "b"}}}}),
            ),
            (
                "another topic",
                false,
                json!({"createTopic": {"topic": "other", "body": {}}}),
            ),
            (
                "a push subscription",
                false,
                json!({"createSubscription": {"subscription": subscription, "body": {
                    "topic": own_topic, "pushConfig": {"pushEndpoint": "https://evil.example"}}}}),
            ),
            (
                "a subscription on another topic",
                false,
                json!({"createSubscription": {"subscription": subscription, "body": {
                    "topic": "projects/acme-project/topics/other"}}}),
            ),
            (
                "an outside publisher",
                true,
                json!({"setTopicIamPolicy": {"topic": topic, "policy": attacker_policy}}),
            ),
            (
                "the storage agent on a topic nothing notifies",
                false,
                json!({"setTopicIamPolicy": {"topic": topic, "policy": agent_policy}}),
            ),
            (
                "an outside subscriber",
                false,
                json!({"setSubscriptionIamPolicy": {"subscription": subscription, "policy": attacker_policy}}),
            ),
            (
                "another subscription",
                false,
                json!({"setSubscriptionIamPolicy": {"subscription": "other-sub", "policy": {}}}),
            ),
        ] {
            let (verifier, backend) = gcp_resource_verifier(&json!({}));
            let reason = refusal(verifier.handle(request(notified, write)).await);
            assert!(reason.starts_with("Refused: "), "{name}: {reason}");
            assert!(backend.forwarded.lock().unwrap().is_empty(), "{name}: sent");
        }
    }

    /// A disk image build is accepted only in the install's own sandbox group, from the signed
    /// image's digest, with the one label that finds it again.
    #[test]
    fn an_azure_sandbox_disk_image_is_checked_against_the_signed_sandbox() {
        use crate::cloud_write::resource::check_sandbox_disk_image;
        use alien_core::{azure_disk_image_label, AZURE_DISK_IMAGE_LABEL};
        let digest = format!("sha256:{}", "a".repeat(64));
        let signed = format!("registry.example.com/vendor/sandbox@{digest}");
        let sandbox = alien_core::Sandbox::new("agent".to_string())
            .code(alien_core::SandboxCode::Image {
                image: signed.clone(),
                bundled_from: None,
            })
            .egress(alien_core::SandboxEgress::None)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let group = format!("{PREFIX}-agent");
        let labels = |base: &str| {
            std::collections::BTreeMap::from([(
                AZURE_DISK_IMAGE_LABEL.to_string(),
                azure_disk_image_label(base),
            )])
        };
        // The signed image, and the same bytes through the manager's pull proxy.
        for base in [
            signed.clone(),
            format!("manager.example.com/proxy/sandbox@{digest}"),
        ] {
            check_sandbox_disk_image(&sandbox, PREFIX, &group, &base, &labels(&base))
                .unwrap_or_else(|error| panic!("{base}: {}", crate::describe(&error)));
        }
        let other_digest = format!(
            "registry.example.com/vendor/sandbox@sha256:{}",
            "b".repeat(64)
        );
        let refused = [
            (
                "another group",
                format!("{PREFIX}-other"),
                signed.clone(),
                labels(&signed),
            ),
            (
                "another digest",
                group.clone(),
                other_digest.clone(),
                labels(&other_digest),
            ),
            (
                "a catalog name",
                group.clone(),
                "ubuntu".to_string(),
                labels("ubuntu"),
            ),
            (
                "an unpinned tag",
                group.clone(),
                "registry.example.com/vendor/sandbox:latest".to_string(),
                labels("registry.example.com/vendor/sandbox:latest"),
            ),
            ("another label", group.clone(), signed.clone(), {
                let mut labels = labels(&signed);
                labels.insert("extra".to_string(), "x".to_string());
                labels
            }),
            (
                "a label for another image",
                group.clone(),
                signed.clone(),
                labels(&other_digest),
            ),
        ];
        for (name, group, base, labels) in refused {
            assert!(
                check_sandbox_disk_image(&sandbox, PREFIX, &group, &base, &labels).is_err(),
                "{name} was accepted"
            );
        }
    }

    /// A sandbox create is accepted only as the signed sandbox declares it: its own group, the
    /// disk image built from the signed digest, the signed size, egress and idle behaviour, no
    /// group network connection, and only the two labels the provider sets. The caller's
    /// environment is free. (Tested on the pure check, which the state check calls once it has
    /// found the image.)
    #[test]
    fn an_azure_sandbox_create_is_checked_against_the_signed_sandbox() {
        use crate::cloud_write::resource::check_sandbox_create;
        use alien_azure_clients::azure::sandbox_data_plane::{
            create_body, egress_policy, CreateSandbox,
        };
        let digest = format!("sha256:{}", "a".repeat(64));
        let signed = format!("registry.example.com/vendor/sandbox@{digest}");
        let egress = alien_core::SandboxEgress::AllowDomains {
            domains: vec!["api.example.com".to_string()],
        };
        let sandbox = alien_core::Sandbox::new("agent".to_string())
            .code(alien_core::SandboxCode::Image {
                image: signed.clone(),
                bundled_from: None,
            })
            .egress(egress.clone())
            .limits(alien_core::SandboxLimits {
                cpu: "500m".to_string(),
                memory: "1024Mi".to_string(),
                disk: "2048Mi".to_string(),
                max_processes: None,
            })
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: Some(600),
            })
            .build();
        let group = format!("{PREFIX}-agent");
        let request = |f: &dyn Fn(&mut CreateSandbox)| {
            let mut request = CreateSandbox {
                disk_image: String::new(),
                disk_image_id: Some("img-1".to_string()),
                cpu: "500m".to_string(),
                memory: "1024Mi".to_string(),
                disk: Some("2048Mi".to_string()),
                environment: [("MODE".to_string(), "x".to_string())].into(),
                egress: egress_policy(&egress),
                customer_vnet_connection_name: None,
                idle_pause_seconds: Some(600),
                labels: [(
                    alien_core::SANDBOX_EXPIRY_LABEL.to_string(),
                    "1700000000".to_string(),
                )]
                .into(),
            };
            f(&mut request);
            create_body(&request)
        };
        let check = |group: &str, base: &str, body: &serde_json::Value, image: Option<&str>| {
            check_sandbox_create(&sandbox, PREFIX, group, base, body, image)
        };
        let signed_body = request(&|_| {});
        check(&group, &signed, &signed_body, None).expect("shape");
        check(&group, &signed, &signed_body, Some("img-1")).expect("with the found image");
        // The same bytes through the manager's pull proxy.
        let proxied = format!("manager.example.com/proxy/sandbox@{digest}");
        check(&group, &proxied, &signed_body, Some("img-1")).expect("proxied base");
        // Any environment is the caller's.
        let own_env = request(&|r| {
            r.environment = [("ANY".to_string(), "thing".to_string())].into();
        });
        check(&group, &signed, &own_env, None).expect("another environment");

        let mut with_field = signed_body.clone();
        with_field["identity"] = json!({"type": "SystemAssigned"});
        let mut other_source = signed_body.clone();
        other_source["sourcesRef"] = json!({"diskImage": {"name": "ubuntu", "isPublic": true}});
        let other_digest = format!(
            "registry.example.com/vendor/sandbox@sha256:{}",
            "b".repeat(64)
        );
        let refused: Vec<(&str, String, String, serde_json::Value, Option<&str>)> = vec![
            (
                "another group",
                format!("{PREFIX}-other"),
                signed.clone(),
                signed_body.clone(),
                None,
            ),
            (
                "another base digest",
                group.clone(),
                other_digest,
                signed_body.clone(),
                None,
            ),
            (
                "a catalog name",
                group.clone(),
                "ubuntu".into(),
                signed_body.clone(),
                None,
            ),
            (
                "a catalog source",
                group.clone(),
                signed.clone(),
                other_source,
                None,
            ),
            (
                "another image id",
                group.clone(),
                signed.clone(),
                signed_body.clone(),
                Some("img-2"),
            ),
            (
                "more cpu",
                group.clone(),
                signed.clone(),
                request(&|r| r.cpu = "4000m".into()),
                None,
            ),
            (
                "more memory",
                group.clone(),
                signed.clone(),
                request(&|r| r.memory = "2048Mi".into()),
                None,
            ),
            (
                "more disk",
                group.clone(),
                signed.clone(),
                request(&|r| r.disk = Some("9999Mi".into())),
                None,
            ),
            (
                "no disk ceiling",
                group.clone(),
                signed.clone(),
                request(&|r| r.disk = None),
                None,
            ),
            (
                "open egress",
                group.clone(),
                signed.clone(),
                request(&|r| r.egress = None),
                None,
            ),
            (
                "wider egress",
                group.clone(),
                signed.clone(),
                request(&|r| {
                    r.egress = egress_policy(&alien_core::SandboxEgress::AllowDomains {
                        domains: vec!["evil.example".to_string()],
                    })
                }),
                None,
            ),
            (
                "a group network connection",
                group.clone(),
                signed.clone(),
                request(&|r| r.customer_vnet_connection_name = Some("default".into())),
                None,
            ),
            (
                "another idle time",
                group.clone(),
                signed.clone(),
                request(&|r| r.idle_pause_seconds = Some(86400)),
                None,
            ),
            (
                "no idle policy",
                group.clone(),
                signed.clone(),
                request(&|r| r.idle_pause_seconds = None),
                None,
            ),
            (
                "a foreign label",
                group.clone(),
                signed.clone(),
                request(&|r| {
                    r.labels.insert("identity".into(), "gateway".into());
                }),
                None,
            ),
            (
                "a non-numeric expiry",
                group.clone(),
                signed.clone(),
                request(&|r| {
                    r.labels
                        .insert(alien_core::SANDBOX_EXPIRY_LABEL.into(), "never".into());
                }),
                None,
            ),
            (
                "an extra field",
                group.clone(),
                signed.clone(),
                with_field,
                None,
            ),
        ];
        for (name, group, base, body, image) in refused {
            assert!(
                check(&group, &base, &body, image).is_err(),
                "{name} was accepted"
            );
        }
    }

    /// A sandbox template is accepted only as the signed sandbox declares it: its own name, its
    /// image (or the same path on an Artifact Registry host), its limits and egress, and no
    /// other field. (Tested on its own: the check is pure.)
    #[test]
    fn a_gcp_sandbox_template_is_checked_against_the_signed_sandbox() {
        use crate::cloud_write::resource::check_sandbox_template;
        let signed = "registry.example.com/vendor/artifacts-prj-abc/prj_abc:sbx-agent";
        let native = "us-central1-docker.pkg.dev/vendor/artifacts-prj-abc/prj_abc:sbx-agent";
        let sandbox = alien_core::Sandbox::new("agent".to_string())
            .code(alien_core::SandboxCode::Image {
                image: signed.to_string(),
                bundled_from: None,
            })
            .egress(alien_core::SandboxEgress::None)
            .lifecycle(alien_core::SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let limits = sandbox.resolved_limits();
        let name = format!("{PREFIX}-agent");
        let template = |image: &str| {
            json!({
                "displayName": name,
                "customContainerEnvironment": {
                    "customContainerSpec": {"imageUri": image},
                    "resources": {"limits": {"cpu": limits.cpu, "memory": limits.memory}},
                    "ports": [{"port": alien_core::sandbox_image::GCP_AGENT_PLATFORM.port,
                               "protocol": "TCP"}]
                },
                "egressControlConfig": {"internetAccess": false}
            })
        };
        let check =
            |engine: &str, body: &Value| check_sandbox_template(&sandbox, PREFIX, engine, body);
        check("123", &template(signed)).expect("the signed template");
        check("123", &template(native)).expect("the signed image on the native host");

        let mut other_name = template(signed);
        other_name["displayName"] = json!("other");
        let mut open_egress = template(signed);
        open_egress["egressControlConfig"] = json!({"internetAccess": true});
        let mut other_limits = template(signed);
        other_limits["customContainerEnvironment"]["resources"]["limits"]["cpu"] = json!("64");
        let mut command = template(signed);
        command["customContainerEnvironment"]["customContainerSpec"]["command"] = json!(["sh"]);
        let mut other_port = template(signed);
        other_port["customContainerEnvironment"]["ports"][0]["port"] = json!(1);
        let mut state = template(signed);
        state["state"] = json!("ACTIVE");
        for (what, engine, body) in [
            (
                "an image of another registry",
                "123",
                template("evil.example.com/vendor/artifacts-prj-abc/prj_abc:sbx-agent"),
            ),
            (
                "another path on an Artifact Registry host",
                "123",
                template("us-central1-docker.pkg.dev/evil/repo/img:1"),
            ),
            ("another display name", "123", other_name),
            ("open egress", "123", open_egress),
            ("other limits", "123", other_limits),
            ("a command", "123", command),
            ("another port", "123", other_port),
            ("a state", "123", state),
            ("an engine path", "1/../2", template(signed)),
        ] {
            check(engine, &body).expect_err(what);
        }
    }

    /// The bucket policy the controller builds is accepted unchanged, public read only when the
    /// signed stack declares the storage public. Anything that would let another principal in is
    /// refused, and nothing goes out.
    #[tokio::test]
    async fn a_bucket_policy_is_checked_against_the_signed_stack_and_sent_unchanged() {
        let bucket = format!("{PREFIX}-assets");
        let arn = format!("arn:aws:s3:::{bucket}");
        let deny = json!({
            "Sid": "DenyInsecureTransport", "Effect": "Deny", "Principal": "*",
            "Action": "s3:*", "Resource": [arn, format!("{arn}/*")],
            "Condition": {"Bool": {"aws:SecureTransport": "false"}}
        });
        let public = alien_permissions::aws_bucket_policy::public_read_statement(&arn);

        let (verifier, backend) = resource_verifier();
        verifier
            .handle(bucket_policy_request(false, &bucket, vec![deny.clone()]))
            .await
            .expect("the private policy is accepted");
        assert_eq!(backend.forwarded.lock().unwrap().len(), 1);
        let sent: Value = serde_json::from_str(&backend.forwarded.lock().unwrap()[0]).unwrap();
        assert_eq!(sent["Statement"], json!([deny.clone()]), "sent unchanged");

        let (verifier, backend) = resource_verifier();
        verifier
            .handle(bucket_policy_request(
                true,
                &bucket,
                vec![deny.clone(), public.clone()],
            ))
            .await
            .expect("public read is accepted for a storage the stack declares public");
        assert_eq!(backend.forwarded.lock().unwrap().len(), 1);

        let foreign = json!({
            "Effect": "Allow", "Principal": {"AWS": "arn:aws:iam::999988887777:root"},
            "Action": "s3:*", "Resource": [arn, format!("{arn}/*")]
        });
        let management = json!({
            "Effect": "Allow",
            "Principal": {"AWS": format!("arn:aws:iam::{ACCOUNT}:role/{PREFIX}-management")},
            "Action": "s3:PutObject", "Resource": format!("{arn}/*")
        });
        let service = json!({
            "Effect": "Allow", "Principal": {"Service": "sns.amazonaws.com"},
            "Action": "s3:PutObject", "Resource": format!("{arn}/*")
        });
        for (name, statements, declared_public) in [
            ("a foreign account", vec![deny.clone(), foreign], true),
            ("the management role", vec![deny.clone(), management], true),
            (
                "a service principal without a source",
                vec![deny.clone(), service],
                true,
            ),
            (
                "a wildcard the stack does not declare",
                vec![deny.clone(), public.clone()],
                false,
            ),
        ] {
            let (verifier, backend) = resource_verifier();
            let reason = refusal(
                verifier
                    .handle(bucket_policy_request(declared_public, &bucket, statements))
                    .await,
            );
            assert!(reason.contains("PutBucketPolicy"), "{name}: {reason}");
            assert!(backend.forwarded.lock().unwrap().is_empty(), "{name}: sent");
        }

        // Another bucket, or a resource the stack does not have, is refused too.
        let (verifier, backend) = resource_verifier();
        let reason = refusal(
            verifier
                .handle(bucket_policy_request(
                    false,
                    "someone-elses-bucket",
                    vec![deny.clone()],
                ))
                .await,
        );
        assert!(reason.contains("not acme-prod-assets"), "{reason}");
        let mut unknown = bucket_policy_request(false, &bucket, vec![deny]);
        unknown["applyResourceWrite"]["resource"] = json!("ledger");
        let reason = refusal(verifier.handle(unknown).await);
        assert!(reason.contains("is not in the signed stack"), "{reason}");
        assert!(backend.forwarded.lock().unwrap().is_empty());
    }

    /// The public access block of a storage the stack does not declare public keeps every
    /// protection; a request that lowers one is refused, and the signed one is sent unchanged.
    #[tokio::test]
    async fn a_public_access_block_is_checked_against_the_signed_stack() {
        let bucket = format!("{PREFIX}-assets");
        let request = |public_read: bool, bucket: &str, configuration: Value| {
            json!({"applyResourceWrite": {
                "release": storage_release(public_read),
                "signerLists": [signer_list()],
                "resource": "assets",
                "writes": [{"putPublicAccessBlock": {
                    "bucket": bucket, "configuration": configuration,
                }}],
            }})
        };
        let all = json!({"BlockPublicAcls": true, "IgnorePublicAcls": true,
                         "BlockPublicPolicy": true, "RestrictPublicBuckets": true});
        let (verifier, backend) = resource_verifier();
        verifier
            .handle(request(false, &bucket, all.clone()))
            .await
            .expect("the signed block is accepted");
        assert_eq!(backend.forwarded.lock().unwrap().len(), 1);

        let mut open = all.clone();
        open["BlockPublicPolicy"] = json!(false);
        let (verifier, backend) = resource_verifier();
        let reason = refusal(verifier.handle(request(false, &bucket, open.clone())).await);
        assert!(reason.contains("BlockPublicPolicy"), "{reason}");
        let reason = refusal(verifier.handle(request(false, "other", all)).await);
        assert!(reason.contains("not acme-prod-assets"), "{reason}");
        assert!(backend.forwarded.lock().unwrap().is_empty());
        verifier
            .handle(request(true, &bucket, open))
            .await
            .expect("a declared public storage may open its block");
    }

    /// An Azure release whose stack has the queues `jobs` and `other` and the storage `uploads`,
    /// which notifies `jobs` for `in/*.json` only.
    fn azure_notification_release() -> alien_signing::SignedStatement {
        let uploads = alien_core::Storage::new("uploads".to_string())
            .notifications(vec![alien_core::StorageNotification::builder()
                .queue(alien_core::ResourceRef::new(
                    alien_core::Queue::RESOURCE_TYPE,
                    "jobs",
                ))
                .prefix("in/".to_string())
                .suffix(".json".to_string())
                .build()])
            .build();
        let stack = alien_core::Stack::new("shop".to_string())
            .add(
                alien_core::Queue::new("jobs".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .add(
                alien_core::Queue::new("other".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .add(uploads, alien_core::ResourceLifecycle::Frozen)
            .build();
        let body = alien_core::release_statement::release_body(
            alien_core::Platform::Azure,
            &stack,
            &alien_core::release_statement::ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        signed_release(1, &body)
    }

    /// An Event Grid subscription is accepted only as the signed storage's notification rule:
    /// its name, filter and queue, in this install's Service Bus, and nothing else.
    #[tokio::test]
    async fn azure_event_subscriptions_are_checked_against_the_signed_stack() {
        let fresh = || {
            let (mut verifier, backend) = resource_verifier();
            *backend.azure.lock().unwrap() = true;
            verifier.install_id = Arc::new(|_| alien_signing::InstallId::Azure {
                subscription: "11111111-2222-3333-4444-555555555555".to_string(),
                resource_group: PREFIX.to_string(),
            });
            (verifier, backend)
        };
        let container = format!("{PREFIX}-uploads");
        let rule = alien_core::StorageNotification::builder()
            .queue(alien_core::ResourceRef::new(
                alien_core::Queue::RESOURCE_TYPE,
                "jobs",
            ))
            .prefix("in/".to_string())
            .suffix(".json".to_string())
            .build();
        let name = alien_core::storage_notifications::azure_subscription_name(&container, &rule);
        let queue_id = |namespace: &str, group: &str, queue: &str| {
            format!(
                "/subscriptions/11111111-2222-3333-4444-555555555555/resourceGroups/{group}/providers/Microsoft.ServiceBus/namespaces/{namespace}/queues/{queue}"
            )
        };
        let signed = |destination: String| {
            json!({"properties": {
                "destination": {"endpointType": "ServiceBusQueue", "properties": {"resourceId": destination}},
                "filter": {
                    "includedEventTypes": ["Microsoft.Storage.BlobCreated"],
                    "subjectBeginsWith": format!("/blobServices/default/containers/{container}/blobs/in/"),
                    "subjectEndsWith": ".json",
                    "isSubjectCaseSensitive": true,
                },
                "eventDeliverySchema": "CloudEventSchemaV1_0",
            }})
        };
        let own_queue = queue_id("bus", PREFIX, &format!("{PREFIX}-jobs"));
        let request = |account: &str, name: &str, body: Value| {
            json!({"applyResourceWrite": {
                "release": azure_notification_release(),
                "signerLists": [signer_list()],
                "resource": "uploads",
                "writes": [{"putEventSubscription": {"account": account, "name": name, "body": body}}],
            }})
        };

        let (verifier, backend) = fresh();
        let body = signed(own_queue.clone());
        verifier
            .handle(request("acmestore", &name, body.clone()))
            .await
            .unwrap_or_else(|error| panic!("the signed rule: {}", crate::describe(&error)));
        assert_eq!(
            *backend.forwarded.lock().unwrap(),
            vec![body.to_string()],
            "the signed subscription is sent unchanged"
        );

        let mut other_filter = signed(own_queue.clone());
        other_filter["properties"]["filter"]["subjectBeginsWith"] = json!(format!(
            "/blobServices/default/containers/{PREFIX}-secrets/blobs/in/"
        ));
        let mut other_types = signed(own_queue.clone());
        other_types["properties"]["filter"]["includedEventTypes"] = json!([
            "Microsoft.Storage.BlobCreated",
            "Microsoft.Storage.BlobDeleted"
        ]);
        let mut webhook = signed(own_queue.clone());
        webhook["properties"]["destination"] = json!({
            "endpointType": "WebHook",
            "properties": {"endpointUrl": "https://attacker.example/events"},
        });
        let mut extra = signed(own_queue.clone());
        extra["properties"]["deadLetterDestination"] = json!({"endpointType": "StorageBlob"});
        let mut no_suffix = signed(own_queue.clone());
        no_suffix["properties"]["filter"]
            .as_object_mut()
            .unwrap()
            .remove("subjectEndsWith");
        let mut advanced = signed(own_queue.clone());
        advanced["properties"]["filter"]["advancedFilters"] = json!([]);
        for (what, account, subscription, body) in [
            (
                "another queue of the stack",
                "acmestore",
                name.as_str(),
                signed(queue_id("bus", PREFIX, &format!("{PREFIX}-other"))),
            ),
            (
                "a queue in another group",
                "acmestore",
                name.as_str(),
                signed(queue_id("bus", "elsewhere", &format!("{PREFIX}-jobs"))),
            ),
            (
                "a queue outside the stack",
                "acmestore",
                name.as_str(),
                signed(queue_id("bus", PREFIX, "rogue")),
            ),
            (
                "a nested destination",
                "acmestore",
                name.as_str(),
                signed(format!("{own_queue}/x")),
            ),
            (
                "another container",
                "acmestore",
                name.as_str(),
                other_filter,
            ),
            ("more event types", "acmestore", name.as_str(), other_types),
            ("a webhook", "acmestore", name.as_str(), webhook),
            ("another field", "acmestore", name.as_str(), extra),
            (
                "a rule without its suffix",
                "acmestore",
                name.as_str(),
                no_suffix,
            ),
            ("advanced filters", "acmestore", name.as_str(), advanced),
            (
                "a name that is not a rule",
                "acmestore",
                "attacker-subscription",
                signed(own_queue.clone()),
            ),
            (
                "an odd account",
                "../x",
                name.as_str(),
                signed(own_queue.clone()),
            ),
        ] {
            let (verifier, backend) = fresh();
            let result = verifier.handle(request(account, subscription, body)).await;
            assert!(result.is_err(), "{what} must be refused");
            assert!(
                backend.forwarded.lock().unwrap().is_empty(),
                "{what}: a refusal sends nothing"
            );
        }
    }

    /// A release whose stack has the queue `jobs` and the storage `uploads`, which notifies it.
    fn queue_release() -> alien_signing::SignedStatement {
        let jobs = alien_core::Queue::new("jobs".to_string()).build();
        let uploads = alien_core::Storage::new("uploads".to_string())
            .notifications(vec![alien_core::StorageNotification::builder()
                .queue(alien_core::ResourceRef::new(
                    alien_core::Queue::RESOURCE_TYPE,
                    "jobs",
                ))
                .build()])
            .build();
        let stack = alien_core::Stack::new("shop".to_string())
            .add(jobs, alien_core::ResourceLifecycle::Frozen)
            .add(uploads, alien_core::ResourceLifecycle::Frozen)
            .build();
        let body = alien_core::release_statement::release_body(
            alien_core::Platform::Aws,
            &stack,
            &alien_core::release_statement::ReleaseOptions {
                channel: CHANNEL.to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        signed_release(1, &body)
    }

    /// Queue writes are checked against the signed stack: the queue's own name and URL, only a
    /// visibility timeout and a policy, and a policy that lets in only the S3 notifications of
    /// the stack's storages that notify this queue.
    #[tokio::test]
    async fn queue_writes_are_checked_against_the_signed_stack() {
        let name = format!("{PREFIX}-jobs");
        let url = format!("https://sqs.{REGION}.amazonaws.com/{ACCOUNT}/{name}");
        let queue_arn = format!("arn:aws:sqs:{REGION}:{ACCOUNT}:{name}");
        let sid = alien_core::storage_notifications::AWS_QUEUE_INGRESS_SID;
        let ingress = |buckets: &[&str], account: &str| {
            alien_permissions::aws_queue_policy::storage_ingress_statement(
                sid,
                &queue_arn,
                account,
                &buckets.iter().map(|b| b.to_string()).collect::<Vec<_>>(),
            )
        };
        let policy = |statement: Value| {
            json!({"Version": "2012-10-17", "Statement": [statement]}).to_string()
        };
        let request = |write: Value| {
            json!({"applyResourceWrite": {
                "release": queue_release(),
                "signerLists": [signer_list()],
                "resource": "jobs",
                "writes": [write],
            }})
        };
        let set = |attributes: Value| {
            request(json!({"setQueueAttributes": {"queueUrl": url, "attributes": attributes}}))
        };
        let own = format!("arn:aws:s3:::{PREFIX}-uploads");

        // What the controller sends is accepted, and goes out unchanged.
        let (verifier, backend) = resource_verifier();
        for write in [
            request(json!({"createQueue": {"queueName": name, "tags": {"deployment": PREFIX}}})),
            set(json!({"VisibilityTimeout": "60"})),
            set(json!({"Policy": policy(ingress(&[&own], ACCOUNT))})),
            set(json!({"Policy": ""})),
        ] {
            verifier
                .handle(write.clone())
                .await
                .unwrap_or_else(|error| panic!("{write}: {}", crate::describe(&error)));
        }
        assert_eq!(backend.forwarded.lock().unwrap().len(), 4);

        let foreign_bucket = ingress(&["arn:aws:s3:::theirs"], ACCOUNT);
        let foreign_account = ingress(&[&own], "999988887777");
        let management = json!({
            "Effect": "Allow",
            "Principal": {"AWS": format!("arn:aws:iam::{ACCOUNT}:role/{PREFIX}-management")},
            "Action": "sqs:*", "Resource": queue_arn
        });
        let service = json!({
            "Effect": "Allow", "Principal": {"Service": "sns.amazonaws.com"},
            "Action": "sqs:SendMessage", "Resource": queue_arn
        });
        let anyone = json!({
            "Effect": "Allow", "Principal": "*", "Action": "sqs:SendMessage",
            "Resource": queue_arn
        });
        let refused_writes = vec![
            (
                "a bucket the stack does not name",
                set(json!({"Policy": policy(foreign_bucket)})),
            ),
            (
                "a foreign account",
                set(json!({"Policy": policy(foreign_account)})),
            ),
            (
                "the management role",
                set(json!({"Policy": policy(management)})),
            ),
            (
                "a service without a source",
                set(json!({"Policy": policy(service)})),
            ),
            ("anyone", set(json!({"Policy": policy(anyone)}))),
            ("a redrive target", set(json!({"RedrivePolicy": "{}"}))),
            (
                "an over-long timeout",
                set(json!({"VisibilityTimeout": "999999"})),
            ),
            (
                "another queue's name",
                request(json!({"createQueue": {"queueName": "other"}})),
            ),
            (
                "an attribute at create",
                request(
                    json!({"createQueue": {"queueName": name, "attributes": {"Policy": "{}"}}}),
                ),
            ),
            (
                "another queue's url",
                request(json!({"setQueueAttributes": {
                    "queueUrl": "https://sqs.us-east-1.amazonaws.com/999988887777/q",
                    "attributes": {"VisibilityTimeout": "1"}}})),
            ),
        ];
        for (what, write) in refused_writes {
            let (verifier, backend) = resource_verifier();
            let reason = refusal(verifier.handle(write).await);
            assert!(!reason.is_empty(), "{what}");
            assert!(backend.forwarded.lock().unwrap().is_empty(), "{what}: sent");
        }
    }

    /// The signer list version the install's ledger records, if any: what accepting a request
    /// writes before its first cloud call.
    fn recorded_release(store: &MemoryStore) -> Option<u64> {
        Ledger::parse(&store.ledgers.lock().unwrap()[PREFIX], PREFIX)
            .unwrap()
            .signer_list
            .map(|entry| entry.sequence)
    }

    /// All of a workload's writes for one release come in one request. Every write is checked
    /// before any goes out, so one that breaks a row sends none and records nothing; a request
    /// whose writes all pass is recorded, then its writes go out in order.
    #[tokio::test]
    async fn a_request_whose_second_write_breaks_a_row_sends_none() {
        let store = Arc::new(MemoryStore::protected(&settings()));
        let backend = Arc::new(Forwards {
            store: store.clone(),
            forwarded: Mutex::new(Vec::new()),
            fail_once: Mutex::new(None),
        });
        let verifier = Verifier {
            own_name: format!("{PREFIX}-verifier"),
            install_id: Arc::new(|prefix| crate::aws::install_id(ACCOUNT, REGION, prefix)),
            store: store.clone(),
            backend: backend.clone(),
            vpc: None,
            machine_network: None,
        };
        let role = crate::plan::tests::place().run_as("execution");
        let request = |code_image: &str| {
            json!({"applyWrite": {
                "release": release(1, function_workload()),
                "signerLists": [signer_list()],
                "workload": "api",
                "writes": [
                    {"createFunction": {
                        "FunctionName": format!("{PREFIX}-api"),
                        "Role": role,
                        "Code": {"ImageUri": format!("{REGISTRY}/api@{DIGEST}")},
                        "PackageType": "Image",
                        "Environment": {"Variables": {"MODE": "signed"}},
                        "Architectures": ["arm64"],
                    }},
                    {"updateFunctionCode": {
                        "functionName": format!("{PREFIX}-api"),
                        "request": {"ImageUri": code_image, "Publish": false},
                    }},
                    {"putFunctionConcurrency": {
                        "functionName": format!("{PREFIX}-api"),
                        "reservedConcurrentExecutions": 5,
                    }},
                ],
            }})
        };

        let unsigned = format!("{REGISTRY}/api@sha256:{}", "e".repeat(64));
        let reason = refusal(verifier.handle(request(&unsigned)).await);
        assert!(
            reason.contains("write 2 of 3 (UpdateFunctionCode) was refused, so none was applied"),
            "{reason}"
        );
        assert!(
            backend.forwarded.lock().unwrap().is_empty(),
            "nothing went out"
        );
        assert_eq!(recorded_release(&store), None, "nothing recorded");

        let answer = verifier
            .handle(request(&format!("{REGISTRY}/api@{DIGEST}")))
            .await
            .unwrap();
        assert_eq!(
            answer,
            json!({
                "release": 1,
                "function": format!("{PREFIX}-api"),
                "operations": ["CreateFunction", "UpdateFunctionCode", "PutFunctionConcurrency"],
                "answers": [
                    {"operation": "CreateFunction"},
                    {"operation": "UpdateFunctionCode"},
                    {"operation": "PutFunctionConcurrency"},
                ],
            })
        );
        assert_eq!(
            *backend.forwarded.lock().unwrap(),
            vec![
                ("CreateFunction".to_string(), Some(1)),
                ("UpdateFunctionCode".to_string(), Some(1)),
                ("PutFunctionConcurrency".to_string(), Some(1)),
            ],
            "the release is recorded, then the writes go out in order"
        );
        assert_eq!(recorded_release(&store), Some(1));

        let mut empty = request("");
        empty["applyWrite"]["writes"] = json!([]);
        let reason = refusal(verifier.handle(empty).await);
        assert!(reason.contains("carries no write"), "{reason}");
    }

    /// A developer input conditional on a deployer input is read against the value the
    /// install's own store holds, so the verifier reads that input too, though no workload env
    /// names it: a customer who turned the feature off gets a workload without its env.
    #[tokio::test]
    async fn a_gating_deployer_input_is_read_from_the_install() {
        let role = crate::plan::tests::place().run_as("execution");
        let config = config(
            1,
            alien_signing::ConfigBody {
                inputs: serde_json::from_value(json!({"auditSink": "https://audit.example"}))
                    .unwrap(),
                ..Default::default()
            },
        );
        let request = |sink: bool| {
            let mut env = json!({"ALIEN_DEPLOYMENT_ID": "dep_1"});
            if sink {
                env["AUDIT_SINK"] = json!("https://audit.example");
            }
            json!({"applyWrite": {
                "release": crate::plan::tests::release_with_gated_input(),
                "config": config,
                "signerLists": [signer_list()],
                "workload": "api",
                "writes": [{"createFunction": {
                    "FunctionName": format!("{PREFIX}-api"),
                    "Role": role,
                    "Code": {"ImageUri": format!("{REGISTRY}/api@{DIGEST}")},
                    "PackageType": "Image",
                    "Environment": {"Variables": env},
                    "Architectures": ["arm64"],
                }}],
            }})
        };
        for (audit, sink_expected) in [("false", false), ("true", true)] {
            let mut store = MemoryStore::protected(&settings());
            store.inputs.insert("audit".to_string(), audit.to_string());
            let store = Arc::new(store);
            let verifier = Verifier {
                own_name: format!("{PREFIX}-verifier"),
                install_id: Arc::new(|prefix| crate::aws::install_id(ACCOUNT, REGION, prefix)),
                store: store.clone(),
                backend: Arc::new(Forwards {
                    store: store.clone(),
                    forwarded: Mutex::new(Vec::new()),
                    fail_once: Mutex::new(None),
                }),
                vpc: None,
                machine_network: None,
            };
            let reason = refusal(verifier.handle(request(!sink_expected)).await);
            assert!(reason.contains("AUDIT_SINK"), "audit={audit}: {reason}");
            verifier.handle(request(sink_expected)).await.unwrap();
        }
    }

    /// The Lambda backend over a mocked Lambda that reads back `function`, recording what it
    /// forwards.
    struct Reading {
        lambda: alien_aws_clients::lambda::MockLambdaApi,
        forwarded: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Backend for Reading {
        async fn binding_reads(
            &self,
            _place: &Place,
            _workload: &Workload,
        ) -> Result<BTreeMap<String, String>> {
            Ok(BTreeMap::new())
        }
        fn check_write(&self, facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
            crate::cloud_write::lambda::check_write(facts, write)
        }
        async fn check_state(&self, facts: &Facts<'_>, writes: &[CloudWrite]) -> Result<()> {
            crate::cloud_write::lambda::check_state(&self.lambda, facts, writes).await
        }
        async fn check_applied(&self, facts: &Facts<'_>, writes: &[CloudWrite]) -> Result<()> {
            crate::cloud_write::lambda::check_applied(&self.lambda, facts, writes).await
        }
        async fn forward(&self, _facts: &Facts<'_>, write: &CloudWrite) -> Result<Value> {
            self.forwarded
                .lock()
                .unwrap()
                .push(write.operation().to_string());
            Ok(Value::Null)
        }
    }

    /// The function Lambda holds, as GetFunction answers: running `image` with `mode` as the
    /// workload's fixed `MODE`.
    fn held(image: &str, mode: &str) -> Value {
        json!({
            "Configuration": {
                "Role": crate::plan::tests::place().run_as("execution"),
                "Environment": {"Variables": {"MODE": mode}},
                "PackageType": "Image",
                "Architectures": ["arm64"],
                "State": "Active",
                "LastUpdateStatus": "Successful",
            },
            "Code": {"ImageUri": image, "ResolvedImageUri": image},
        })
    }

    /// A request for release 2, which changes the image and the fixed `MODE`, that updates
    /// the image only is refused before it writes anything while the function still holds the
    /// old `MODE`; and when the function doesn't hold the release after the writes, the
    /// request fails instead of reporting success.
    #[tokio::test]
    async fn an_image_only_request_leaves_no_old_env_behind() {
        let new_image = format!("{REGISTRY}/api@sha256:{}", "7".repeat(64));
        let old_image = format!("{REGISTRY}/api@{DIGEST}");
        let mut workload = function_workload_with(&new_image);
        workload
            .env
            .insert("MODE".to_string(), "changed".to_string());
        let request = json!({"applyWrite": {
            "release": release(2, workload),
            "signerLists": [signer_list()],
            "workload": "api",
            "writes": [{"updateFunctionCode": {
                "functionName": format!("{PREFIX}-api"),
                "request": {"ImageUri": new_image},
            }}],
        }});
        let run = |held_before: Value, held_after: Value| {
            let request = request.clone();
            async move {
                let store = Arc::new(MemoryStore::protected(&settings()));
                let reads = Mutex::new(vec![held_after, held_before]);
                let mut lambda = alien_aws_clients::lambda::MockLambdaApi::new();
                lambda
                    .expect_get_function_json()
                    .returning(move |_| Ok(reads.lock().unwrap().pop().unwrap()));
                let backend = Arc::new(Reading {
                    lambda,
                    forwarded: Mutex::new(Vec::new()),
                });
                let verifier = Verifier {
                    own_name: format!("{PREFIX}-verifier"),
                    install_id: Arc::new(|prefix| crate::aws::install_id(ACCOUNT, REGION, prefix)),
                    store: store.clone(),
                    backend: backend.clone(),
                    vpc: None,
                    machine_network: None,
                };
                let answer = verifier.handle(request).await;
                let forwarded = backend.forwarded.lock().unwrap().clone();
                (answer, forwarded, recorded_release(&store))
            }
        };

        let (answer, forwarded, recorded) = run(held(&old_image, "signed"), Value::Null).await;
        let reason = refusal(answer);
        assert!(
            reason.contains("leaves the function's configuration as it is")
                && reason.contains("field /Environment"),
            "{reason}"
        );
        assert!(forwarded.is_empty(), "nothing went out");
        assert_eq!(recorded, None, "nothing was recorded");

        let (answer, forwarded, recorded) =
            run(held(&old_image, "changed"), held(&new_image, "changed")).await;
        assert_eq!(answer.unwrap()["operations"], json!(["UpdateFunctionCode"]));
        assert_eq!(forwarded, vec!["UpdateFunctionCode".to_string()]);
        assert_eq!(recorded, Some(1));

        // The write went out, and Lambda then holds the old env: not reported as done.
        let (answer, forwarded, _) =
            run(held(&old_image, "changed"), held(&new_image, "signed")).await;
        let error = answer.unwrap_err();
        assert_eq!(error.code, "VERIFIER_READ_BACK_MISMATCH", "{error}");
        assert_eq!(forwarded, vec!["UpdateFunctionCode".to_string()]);
    }

    /// A cloud error partway through a recorded release leaves some writes out; the control
    /// plane applies the same release again, which passes and sends every write.
    #[tokio::test]
    async fn a_release_a_cloud_error_cut_short_applies_again_in_full() {
        let store = Arc::new(MemoryStore::protected(&settings()));
        let backend = Arc::new(Forwards {
            store: store.clone(),
            forwarded: Mutex::new(Vec::new()),
            fail_once: Mutex::new(Some("UpdateFunctionCode")),
        });
        let verifier = Verifier {
            own_name: format!("{PREFIX}-verifier"),
            install_id: Arc::new(|prefix| crate::aws::install_id(ACCOUNT, REGION, prefix)),
            store: store.clone(),
            backend: backend.clone(),
            vpc: None,
            machine_network: None,
        };
        let request = json!({"applyWrite": {
            "release": release(1, function_workload()),
            "signerLists": [signer_list()],
            "workload": "api",
            "writes": [
                {"createFunction": {
                    "FunctionName": format!("{PREFIX}-api"),
                    "Role": crate::plan::tests::place().run_as("execution"),
                    "Code": {"ImageUri": format!("{REGISTRY}/api@{DIGEST}")},
                    "PackageType": "Image",
                    "Environment": {"Variables": {"MODE": "signed"}},
                    "Architectures": ["arm64"],
                }},
                {"updateFunctionCode": {
                    "functionName": format!("{PREFIX}-api"),
                    "request": {"ImageUri": format!("{REGISTRY}/api@{DIGEST}"), "Publish": false},
                }},
            ],
        }});

        let error = verifier.handle(request.clone()).await.unwrap_err();
        assert_eq!(error.code, "VERIFIER_CLOUD_CALL_FAILED", "{error}");
        assert_eq!(
            *backend.forwarded.lock().unwrap(),
            vec![("CreateFunction".to_string(), Some(1))],
            "the first write went out before the cloud failed the second"
        );
        assert_eq!(
            recorded_release(&store),
            Some(1),
            "the release was recorded"
        );

        let answer = verifier.handle(request).await.unwrap();
        assert_eq!(answer["release"], json!(1));
        assert_eq!(
            *backend.forwarded.lock().unwrap(),
            vec![
                ("CreateFunction".to_string(), Some(1)),
                ("CreateFunction".to_string(), Some(1)),
                ("UpdateFunctionCode".to_string(), Some(1)),
            ],
            "the same release applies again, every write this time"
        );
        assert_eq!(recorded_release(&store), Some(1));
    }

    #[tokio::test]
    async fn a_caller_named_target_is_refused() {
        // Attack: ask for 30 machines under the small group's bounds while
        // naming the big group's MIG as the target.
        let (verifier, backend, mut request) = gcp_verifier();
        request["scale"]["target"] = json!("acme-prod-compute-big");
        let reason = refusal(verifier.handle(request).await);
        assert!(reason.contains("unknown field `target`"), "{reason}");
        assert!(backend.0.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_group_name_that_leaves_its_path_segment_is_refused() {
        let (verifier, backend, mut request) = gcp_verifier();
        request["scale"]["group"] = json!("../big");
        let reason = refusal(verifier.handle(request).await);
        assert!(
            reason.contains("compute cluster 'compute' has no capacity group '../big'"),
            "{reason}"
        );
        assert!(backend.0.lock().unwrap().is_empty());
    }

    /// A backend that records each workload delete.
    #[derive(Default)]
    struct Deletes(Mutex<Vec<String>>);

    #[async_trait]
    impl Backend for Deletes {
        async fn binding_reads(
            &self,
            _place: &Place,
            _workload: &Workload,
        ) -> Result<BTreeMap<String, String>> {
            Ok(BTreeMap::new())
        }
        async fn delete_workload(&self, name: &str) -> Result<bool> {
            self.0.lock().unwrap().push(name.to_string());
            Ok(true)
        }
        async fn delete_machines(&self, target: &str, zone: Option<&str>) -> Result<bool> {
            self.0
                .lock()
                .unwrap()
                .push(format!("{target} zone={zone:?}"));
            Ok(false)
        }
    }

    /// Uninstall deletes a Live workload by the name the install gives it, with no statement,
    /// and never the verifier or a name outside the install's own.
    #[tokio::test]
    async fn a_delete_removes_only_the_installs_own_workload_and_never_the_verifier() {
        let list = signer_list_of(PROJECT, &ci(), Utc::now());
        let backend = Arc::new(Deletes::default());
        let verifier = Verifier {
            own_name: format!("{PREFIX}-verifier"),
            install_id: Arc::new(|prefix| InstallId::Gcp {
                project: "shop-prod".to_string(),
                prefix: prefix.to_string(),
            }),
            store: Arc::new(MemoryStore::protected(&settings_for(
                &list, PREFIX, CHANNEL,
            ))),
            backend: backend.clone(),
            vpc: None,
            machine_network: None,
        };

        let answer = verifier
            .handle(json!({"deleteWorkload": {"workload": "api"}}))
            .await
            .unwrap();
        assert_eq!(answer, json!({"deleted": "acme-prod-api", "existed": true}));

        for (workload, why) in [
            ("verifier", "would be the install's verifier"),
            ("api/../other", "is not one setup creates as is"),
            ("api?x=1", "is not one setup creates as is"),
            ("", "is not one setup creates as is"),
        ] {
            let reason = refusal(
                verifier
                    .handle(json!({"deleteWorkload": {"workload": workload}}))
                    .await,
            );
            assert!(reason.contains(why), "{workload}: {reason}");
        }
        let reason = refusal(
            verifier
                .handle(
                    json!({"deleteWorkload": {"workload": "api", "name": "acme-prod-verifier"}}),
                )
                .await,
        );
        assert!(reason.contains("unknown field"), "{reason}");
        assert_eq!(*backend.0.lock().unwrap(), ["acme-prod-api"]);
    }

    /// Uninstall deletes a machine group by the name the install gives it, with no statement:
    /// the caller names only the signed cluster and group ids, never the cloud resource.
    #[tokio::test]
    async fn a_machines_delete_names_only_the_installs_own_group() {
        let list = signer_list_of(PROJECT, &ci(), Utc::now());
        let backend = Arc::new(Deletes::default());
        let verifier = Verifier {
            own_name: format!("{PREFIX}-verifier"),
            install_id: Arc::new(|prefix| InstallId::Gcp {
                project: "shop-prod".to_string(),
                prefix: prefix.to_string(),
            }),
            store: Arc::new(MemoryStore::protected(&settings_for(
                &list, PREFIX, CHANNEL,
            ))),
            backend: backend.clone(),
            vpc: None,
            machine_network: None,
        };

        let answer = verifier
            .handle(json!({"deleteMachines": {"cluster": "compute", "group": "general", "zone": "us-central1-a"}}))
            .await
            .unwrap();
        assert_eq!(
            answer,
            json!({"group": "acme-prod-compute-general", "gone": false})
        );

        for (cluster, group) in [
            ("compute", "general/../x"),
            ("compute", ""),
            ("a b", "general"),
        ] {
            let reason = refusal(
                verifier
                    .handle(json!({"deleteMachines": {"cluster": cluster, "group": group}}))
                    .await,
            );
            assert!(
                reason.contains("may not be part of a machine group name"),
                "{cluster}/{group}: {reason}"
            );
        }
        let reason = refusal(
            verifier
                .handle(json!({"deleteMachines": {"cluster": "compute", "group": "general", "name": "acme-prod-verifier"}}))
                .await,
        );
        assert!(reason.contains("unknown field"), "{reason}");
        assert_eq!(
            *backend.0.lock().unwrap(),
            ["acme-prod-compute-general zone=Some(\"us-central1-a\")"]
        );
    }
}
