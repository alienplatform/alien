//! One verifier call, from the request to the writes, for any cloud.

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_error::{AlienError, Context};
use alien_signing::{InstallId, Trust, Workload};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use tracing::info;

use crate::error::{ErrorData, Result};
use crate::machines::{self, RollMachines, Scale};
use crate::plan::{
    functions, plan_function, FunctionSpec, Place, Reads, ReleaseRequest, Request, SetupNetwork,
};
use crate::trust::{accept, find_install, store_ledger, Install, InstallStore};

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
    /// Write `spec` and read it back.
    async fn apply(&self, spec: &FunctionSpec) -> Result<()>;
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
        for name in request.provided_env.keys().chain(request.images.keys()) {
            if !functions(&accepted.release.body).any(|workload| &workload.name == name) {
                return Err(refused(format!(
                    "workload '{name}' is not a function of the signed release"
                )));
            }
        }
        // Build every function before writing any, so a refusal changes nothing.
        let mut specs = Vec::new();
        for workload in functions(&accepted.release.body) {
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
        if write {
            store_ledger(self.store.as_ref(), &place.prefix, &accepted.ledger).await?;
        }
        for spec in &specs {
            if write {
                self.backend.apply(spec).await?;
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

    async fn scale(&self, request: &Scale) -> Result<Value> {
        let (install, place) = self.install().await?;
        let now = Utc::now();
        let trust = Trust::new(
            install.settings.clone(),
            install.id.clone(),
            &request.signer_lists,
            &install.ledger,
            now,
        )
        .context(ErrorData::Refused {
            reason: "the signer lists were refused".to_string(),
        })?;
        let release = trust
            .verify_release(&request.release, now)
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
        let group = machines::signed_group(&release.body, &request.cluster, &request.group)?;
        let target = machines::scale_target(&place.prefix, &request.cluster, &request.group)?;
        let size = machines::clamp(request.desired, &group);
        self.backend
            .resize(&target, request.zone.as_deref(), size)
            .await?;
        info!(target = %target, desired = request.desired, size, "resized within the signed bounds");
        Ok(json!({"target": target, "size": size}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::trust::tests::MemoryStore;
    use alien_core::release_statement::{release_body, ReleaseOptions};
    use alien_core::{CapacityGroup, ComputeCluster, Platform, ResourceLifecycle, Stack};
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
        async fn apply(&self, _spec: &FunctionSpec) -> Result<()> {
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
