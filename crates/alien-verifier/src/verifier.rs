//! One verifier call, from the request to the writes, for any cloud.

use std::collections::BTreeMap;
use std::sync::Arc;

use alien_error::{AlienError, Context};
use alien_signing::{Entry, InstallId, Trust, Workload};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use tracing::info;

use crate::cloud_write::{own_workload, ApplyWrite, CloudWrite, Facts};
use crate::error::{ErrorData, Result};
use crate::function::StillNewest;
use crate::machines::{self, RemoveMachines, RollMachines, Scale};
use crate::plan::{Place, Reads, Request, SetupNetwork};
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
    /// Check every field of `write` against this cloud's table.
    fn check_write(&self, _facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
        Err(refused(format!(
            "this cloud takes no {} write",
            write.operation()
        )))
    }
    /// Send `write`, already checked, to the cloud unchanged, checking `newest` right before,
    /// and read back what decides what runs. Answers what the cloud answered.
    async fn forward(
        &self,
        _facts: &Facts<'_>,
        write: &CloudWrite,
        _newest: &dyn StillNewest,
    ) -> Result<Value> {
        Err(refused(format!(
            "this cloud takes no {} write",
            write.operation()
        )))
    }
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
            Request::ApplyWrite(request) => self.write(&request).await,
            Request::RollMachines(request) => self.roll(&request).await,
            Request::Scale(request) => self.scale(&request).await,
            Request::RemoveMachines(request) => self.remove(&request).await,
        }
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
            ..Reads::default()
        };
        for input in workload.deployer_env.values() {
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
        // Every field is checked before anything is written.
        self.backend.check_write(&facts, &request.write)?;
        store_ledger(self.store.as_ref(), &place.prefix, &accepted.ledger).await?;
        let floor = Floor {
            store: self.store.as_ref(),
            prefix: &place.prefix,
            release: Entry {
                sequence: accepted.release.sequence,
                digest: accepted.release.digest.clone(),
            },
        };
        let answer = self.backend.forward(&facts, &request.write, &floor).await?;
        info!(
            function = %facts.own_name,
            operation = request.write.operation(),
            "applied the controller's checked write"
        );
        Ok(json!({
            "release": accepted.release.sequence,
            "function": facts.own_name,
            "operation": request.write.operation(),
            "answer": answer,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::trust::tests::MemoryStore;
    use alien_core::release_statement::{release_body, ReleaseOptions};
    use alien_core::{CapacityGroup, ComputeCluster, Platform, ResourceLifecycle, Stack};
    use alien_signing::{Kind, Ledger};
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

    /// An AWS backend that checks writes with the Lambda table and records what it forwards.
    #[derive(Default)]
    struct Forwards(Mutex<Vec<CloudWrite>>);

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
        async fn forward(
            &self,
            _facts: &Facts<'_>,
            write: &CloudWrite,
            newest: &dyn StillNewest,
        ) -> Result<Value> {
            newest.check().await?;
            self.0.lock().unwrap().push(write.clone());
            Ok(Value::Null)
        }
    }

    /// The statements are accepted and recorded before the controller's write goes out; a write
    /// a row refuses goes nowhere and records nothing.
    #[tokio::test]
    async fn a_checked_write_is_forwarded_after_the_ledger_and_a_refused_one_is_not() {
        let backend = Arc::new(Forwards::default());
        let store = Arc::new(MemoryStore::protected(&settings()));
        let verifier = Verifier {
            own_name: format!("{PREFIX}-verifier"),
            install_id: Arc::new(|prefix| crate::aws::install_id(ACCOUNT, REGION, prefix)),
            store: store.clone(),
            backend: backend.clone(),
            vpc: None,
            machine_network: None,
        };
        let write = |role: &str| {
            json!({"applyWrite": {
                "release": release(1, function_workload()),
                "signerLists": [signer_list()],
                "workload": "api",
                "write": {"createFunction": {
                    "FunctionName": format!("{PREFIX}-api"),
                    "Role": role,
                    "Code": {"ImageUri": format!("{REGISTRY}/api@{DIGEST}")},
                    "PackageType": "Image",
                    "Environment": {"Variables": {"MODE": "signed"}},
                    "Architectures": ["arm64"],
                }},
            }})
        };
        let ledger = |store: &MemoryStore| {
            Ledger::parse(&store.ledgers.lock().unwrap()[PREFIX], PREFIX).unwrap()
        };

        let refused = verifier
            .handle(write(&format!("arn:aws:iam::{ACCOUNT}:role/admin")))
            .await
            .unwrap_err();
        assert!(
            refused.to_string().contains("field /Role must match"),
            "{refused}"
        );
        assert!(backend.0.lock().unwrap().is_empty(), "nothing went out");
        assert!(
            ledger(&store).floor(Kind::Release).unwrap().is_none(),
            "nothing recorded"
        );

        let role = crate::plan::tests::place().run_as("execution");
        let answer = verifier.handle(write(&role)).await.unwrap();
        assert_eq!(
            answer,
            json!({"release": 1, "function": format!("{PREFIX}-api"), "operation": "CreateFunction", "answer": null})
        );
        assert_eq!(backend.0.lock().unwrap().len(), 1);
        assert_eq!(
            ledger(&store)
                .floor(Kind::Release)
                .unwrap()
                .unwrap()
                .sequence,
            1
        );
    }

    /// A backend whose forward of an UpdateFunctionCode to `slow_image` pauses after reading
    /// the function, until told to go on, the way a Lambda update waits for the function to
    /// settle.
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
        fn check_write(&self, facts: &Facts<'_>, write: &CloudWrite) -> Result<()> {
            crate::cloud_write::lambda::check_write(facts, write)
        }
        async fn forward(
            &self,
            _facts: &Facts<'_>,
            write: &CloudWrite,
            newest: &dyn StillNewest,
        ) -> Result<Value> {
            let CloudWrite::UpdateFunctionCode { request, .. } = write else {
                unreachable!("the test sends only UpdateFunctionCode")
            };
            let image = request["ImageUri"].as_str().unwrap().to_string();
            if image == self.slow_image {
                self.read.notify_one();
                self.resume.notified().await;
            }
            newest.check().await?;
            self.written.lock().unwrap().push(image);
            Ok(Value::Null)
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
            json!({"applyWrite": {
                "release": signed_release(sequence, &crate::fixtures::release_body(vec![workload])),
                "signerLists": [signer_list()],
                "workload": "api",
                "write": {"updateFunctionCode": {
                    "functionName": format!("{PREFIX}-api"),
                    "request": {"ImageUri": image(digit), "Publish": true},
                }},
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
