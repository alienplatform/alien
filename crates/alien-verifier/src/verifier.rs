//! One verifier call, from the request to the writes, for any cloud.

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_error::{AlienError, Context};
use alien_signing::{Entry, InstallId, Trust, Workload};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use tracing::info;

use crate::error::{ErrorData, Result};
use crate::function::StillNewest;
use crate::machines::{self, RemoveMachines, RollMachines, Scale};
use crate::plan::{
    functions, plan_function, FunctionSpec, Place, Reads, ReleaseRequest, Request, SetupNetwork,
};
use crate::trust::{accept, find_install, store_ledger, Floor, Install, InstallStore};

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
    /// Write `spec` and read it back, checking `newest` before each write.
    async fn apply(
        &self,
        place: &Place,
        spec: &FunctionSpec,
        newest: &dyn StillNewest,
    ) -> Result<()>;
    /// Read `spec` back.
    async fn check(&self, spec: &FunctionSpec) -> Result<()>;
    /// Roll a machine group to the signed release.
    async fn roll(
        &self,
        _place: &Place,
        _release: &alien_signing::ReleaseBody,
        _request: &RollMachines,
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
            Request::ApplyRelease(request) => self.release(&request, true).await,
            Request::CheckRelease(request) => self.release(&request, false).await,
            Request::RollMachines(request) => self.roll(&request).await,
            Request::Scale(request) => self.scale(&request).await,
            Request::RemoveMachines(request) => self.remove(&request).await,
        }
    }

    async fn release(&self, request: &ReleaseRequest, write: bool) -> Result<Value> {
        let (install, place) = self.install().await?;
        let accepted = accept(
            &install,
            &request.signer_lists,
            &request.release,
            request.config.as_ref(),
            place.platform(),
            Utc::now(),
        )?;
        let named = request.workloads.iter().flatten();
        for name in request
            .provided_env
            .keys()
            .chain(request.images.keys())
            .chain(named)
        {
            if !functions(&accepted.release.body).any(|workload| &workload.name == name) {
                return Err(refused(format!(
                    "workload '{name}' is not a function of the signed release"
                )));
            }
            if !request.selects(name) {
                return Err(refused(format!(
                    "workload '{name}' has values but is not one of the workloads to write"
                )));
            }
        }
        // Build every function before writing any, so a refusal changes nothing.
        let mut specs = Vec::new();
        for workload in functions(&accepted.release.body).filter(|w| request.selects(&w.name)) {
            let mut reads = Reads {
                bindings: self.backend.binding_reads(&place, workload).await?,
                ..Reads::default()
            };
            for input in workload.deployer_env.values() {
                if let Some(value) = self.store.input(&place.prefix, input).await? {
                    reads.deployer_inputs.insert(input.clone(), value);
                }
            }
            specs.push(plan_function(&place, &accepted, request, workload, &reads)?);
        }
        for spec in &specs {
            own_name(&place, spec)?;
        }
        if write {
            store_ledger(self.store.as_ref(), &place.prefix, &accepted.ledger).await?;
        }
        let floor = Floor {
            store: self.store.as_ref(),
            prefix: &place.prefix,
            release: Entry {
                sequence: accepted.release.sequence,
                digest: accepted.release.digest.clone(),
            },
        };
        for spec in &specs {
            if write {
                self.backend.apply(&place, spec, &floor).await?;
                info!(function = %spec.name, digest = %spec.digest, "applied the signed function");
            } else {
                self.backend.check(spec).await?;
            }
        }
        Ok(json!({
            "release": accepted.release.sequence,
            "functions": specs
                .iter()
                .map(|spec| json!({"function": spec.name, "imageUri": spec.image_uri, "digest": spec.digest}))
                .collect::<Vec<_>>(),
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
        // Check before writing the ledger; the backend refuses with the reason.
        let rolled = self
            .backend
            .roll(&place, &accepted.release.body, request)
            .await?;
        store_ledger(self.store.as_ref(), &place.prefix, &accepted.ledger).await?;
        Ok(rolled)
    }

    /// The release a scale or removal reads the signed bounds from: verified,
    /// or the one kept at the install's floor, and no older than the floor.
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
        let release = crate::trust::verify_or_keep_release(&trust, &install.ledger, release, now)
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
        machines::check_floor(&install.ledger, release.sequence, &release.digest)?;
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

/// The verifier writes only `<prefix>-<workload>`: its own install's name for a function of the
/// signed release. Its grants on Cloud Run reach every service in the project, so this check is
/// the boundary: any other name is refused before the ledger or the cloud is written.
fn own_name(place: &Place, spec: &FunctionSpec) -> Result<()> {
    let own = place.function_name(&spec.workload)?;
    if spec.name != own || !spec.name.starts_with(&format!("{}-", place.prefix)) {
        return Err(refused(format!(
            "function {} is not {own}, the name this install gives workload '{}'",
            spec.name, spec.workload
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::trust::tests::MemoryStore;
    use alien_core::release_statement::{release_body, ReleaseOptions};
    use alien_core::{CapacityGroup, ComputeCluster, Platform, ResourceLifecycle, Stack};
    use std::sync::Mutex;

    /// The verifier writes only its own install's name for a signed workload; a spec naming
    /// another service is refused before anything is written, whatever built it.
    #[test]
    fn only_the_installs_own_names_are_written() {
        let place = crate::plan::tests::place();
        let spec = |name: &str| FunctionSpec {
            workload: "api".into(),
            name: name.into(),
            image_uri: format!("{REGISTRY}/api@{DIGEST}"),
            digest: DIGEST.into(),
            env: Default::default(),
            memory_mb: 512,
            timeout_seconds: 30,
            architecture: "arm64".into(),
            run_as: "arn:aws:iam::111122223333:role/acme-prod-execution-sa".into(),
            vpc: None,
            public: false,
            dapr: false,
            registry: None,
            reserved_concurrency: None,
            invoke_grants: Vec::new(),
        };
        own_name(&place, &spec(&format!("{PREFIX}-api"))).unwrap();
        for other in [
            "other-install-api",
            "acme-prod-other",
            "acme-api",
            "acme-prod-api-2",
        ] {
            let error = own_name(&place, &spec(other)).unwrap_err();
            assert_eq!(error.code, "VERIFIER_REFUSED", "{other}");
            assert!(
                error
                    .to_string()
                    .contains(&format!("function {other} is not {PREFIX}-api")),
                "{error}"
            );
        }
    }

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
        async fn apply(
            &self,
            _place: &Place,
            _spec: &FunctionSpec,
            _newest: &dyn StillNewest,
        ) -> Result<()> {
            unreachable!("scale writes no function")
        }
        async fn check(&self, _spec: &FunctionSpec) -> Result<()> {
            unreachable!("scale reads no function")
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

    /// An AWS backend that records the functions it writes.
    #[derive(Default)]
    struct Writes(Mutex<Vec<FunctionSpec>>);

    #[async_trait]
    impl Backend for Writes {
        async fn binding_reads(
            &self,
            _place: &Place,
            _workload: &Workload,
        ) -> Result<BTreeMap<String, String>> {
            Ok(BTreeMap::new())
        }
        async fn apply(
            &self,
            _place: &Place,
            spec: &FunctionSpec,
            newest: &dyn StillNewest,
        ) -> Result<()> {
            self.0.lock().unwrap().push(spec.clone());
            Ok(())
        }
        async fn check(&self, _spec: &FunctionSpec) -> Result<()> {
            unreachable!("applyRelease reads back through apply")
        }
    }

    /// An AWS verifier and a release with two functions, `api` and `jobs`.
    fn two_functions() -> (Verifier, Arc<Writes>, Value) {
        let backend = Arc::new(Writes::default());
        let verifier = Verifier {
            own_name: format!("{PREFIX}-verifier"),
            install_id: Arc::new(|prefix| crate::aws::install_id(ACCOUNT, REGION, prefix)),
            store: Arc::new(MemoryStore::protected(&settings())),
            backend: backend.clone(),
            vpc: None,
            machine_network: None,
        };
        let jobs = alien_signing::Workload {
            name: "jobs".to_string(),
            ..function_workload()
        };
        let release = signed_release(
            1,
            &crate::fixtures::release_body(vec![function_workload(), jobs]),
        );
        let request = json!({"applyRelease": {
            "release": release,
            "signerLists": [signer_list()],
        }});
        (verifier, backend, request)
    }

    #[tokio::test]
    async fn apply_release_writes_only_the_workloads_it_names() {
        let (verifier, backend, mut request) = two_functions();
        request["applyRelease"]["workloads"] = json!(["jobs"]);
        request["applyRelease"]["providedEnv"] = json!({"jobs": {"ALIEN_DEPLOYMENT_ID": "dep_1"}});
        let answer = verifier.handle(request).await.unwrap();
        assert_eq!(answer["release"], json!(1));
        let written = backend.0.lock().unwrap().clone();
        assert_eq!(
            written
                .iter()
                .map(|spec| spec.name.as_str())
                .collect::<Vec<_>>(),
            vec!["acme-prod-jobs"]
        );
        assert_eq!(
            written[0]
                .env
                .get("ALIEN_DEPLOYMENT_ID")
                .map(String::as_str),
            Some("dep_1")
        );
    }

    #[tokio::test]
    async fn apply_release_without_workloads_writes_every_function() {
        let (verifier, backend, request) = two_functions();
        verifier.handle(request).await.unwrap();
        assert_eq!(
            backend
                .0
                .lock()
                .unwrap()
                .iter()
                .map(|spec| spec.name.clone())
                .collect::<Vec<_>>(),
            vec!["acme-prod-api", "acme-prod-jobs"]
        );
    }

    #[tokio::test]
    async fn values_for_a_workload_it_does_not_write_are_refused() {
        // Values for `api` sent with a request that writes only `jobs` would be dropped
        // silently; refuse them, and a name the release has no function for.
        let (verifier, backend, mut request) = two_functions();
        request["applyRelease"]["workloads"] = json!(["jobs"]);
        request["applyRelease"]["providedEnv"] = json!({"api": {"ALIEN_DEPLOYMENT_ID": "dep_1"}});
        let reason = refusal(verifier.handle(request.clone()).await);
        assert!(
            reason.contains("workload 'api' has values but is not one of the workloads to write"),
            "{reason}"
        );
        request["applyRelease"]["providedEnv"] = json!({});
        request["applyRelease"]["workloads"] = json!(["worker"]);
        let reason = refusal(verifier.handle(request).await);
        assert!(
            reason.contains("workload 'worker' is not a function of the signed release"),
            "{reason}"
        );
        assert!(backend.0.lock().unwrap().is_empty());
    }

    /// A backend whose write of `slow_image` pauses after reading the function, until told to
    /// go on, the way a Lambda update waits for the function to settle.
    struct Paused {
        slow_image: String,
        read: tokio::sync::Notify,
        resume: tokio::sync::Notify,
        written: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Backend for Paused {
        async fn binding_reads(
            &self,
            _place: &Place,
            _workload: &Workload,
        ) -> Result<BTreeMap<String, String>> {
            Ok(BTreeMap::new())
        }
        async fn apply(
            &self,
            _place: &Place,
            spec: &FunctionSpec,
            newest: &dyn StillNewest,
        ) -> Result<()> {
            if spec.image_uri == self.slow_image {
                self.read.notify_one();
                self.resume.notified().await;
            }
            newest.check().await?;
            self.written.lock().unwrap().push(spec.image_uri.clone());
            Ok(())
        }
        async fn check(&self, _spec: &FunctionSpec) -> Result<()> {
            unreachable!("applyRelease reads back through apply")
        }
    }

    #[tokio::test]
    async fn an_older_release_applied_concurrently_never_lands_over_a_newer_one() {
        // Release 10 is accepted and starts writing; release 11 is accepted and written while
        // 10's write waits; then 10's write goes on. It must not land over 11.
        let image =
            |digit: char| format!("ghcr.io/shop/api@sha256:{}", digit.to_string().repeat(64));
        let request = |sequence: u64, digit: char| {
            let workload = alien_signing::Workload {
                image: image(digit),
                ..function_workload()
            };
            json!({"applyRelease": {
                "release": signed_release(sequence, &crate::fixtures::release_body(vec![workload])),
                "signerLists": [signer_list()],
            }})
        };
        let backend = Arc::new(Paused {
            slow_image: image('a'),
            read: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
            written: Mutex::new(Vec::new()),
        });
        let store = Arc::new(MemoryStore::protected(&settings()));
        let verifier = Verifier {
            own_name: format!("{PREFIX}-verifier"),
            install_id: Arc::new(|prefix| crate::aws::install_id(ACCOUNT, REGION, prefix)),
            store: store.clone(),
            backend: backend.clone(),
            vpc: None,
            machine_network: None,
        };
        let older = verifier.handle(request(10, 'a'));
        let newer = async {
            backend.read.notified().await;
            let answer = verifier.handle(request(11, 'b')).await;
            backend.resume.notify_one();
            answer
        };
        let (older, newer) = tokio::join!(older, newer);

        assert_eq!(newer.unwrap()["release"], json!(11));
        let reason = refusal(older);
        assert!(
            reason.contains("release 11 was accepted while release 10 was being applied"),
            "{reason}"
        );
        assert_eq!(*backend.written.lock().unwrap(), vec![image('b')]);
        let ledger = match store.ledger(PREFIX).await.unwrap() {
            alien_signing::Lookup::Found(bytes) => {
                alien_signing::Ledger::parse(&bytes, PREFIX).unwrap()
            }
            other => panic!("the ledger is gone: {other:?}"),
        };
        assert_eq!(ledger.release.unwrap().sequence, 11);
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
}
