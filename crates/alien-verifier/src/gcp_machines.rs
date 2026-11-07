//! Machines on protected GCP installs: the verifier writes each instance
//! template and points the managed instance group (MIG) at it.
//!
//! A template is immutable, so every roll adds `<prefix>-<cluster>-<group>-<n>`
//! with the next `n`. The MIG `<prefix>-<cluster>-<group>` (zonal in the
//! request's zone, regional otherwise) is created at the signed minimum, or
//! pointed at the new template with an opportunistic update policy: machines
//! already running keep their template until the platform replaces them, as
//! on AWS. Machines boot from the publisher's image, with setup's subnetwork,
//! network tags and service account, and read their capacity group from
//! instance metadata.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use alien_error::AlienError;
use alien_signing::ReleaseBody;

use crate::error::{ErrorData, Result};
use crate::machines::{
    check_setup_ids, check_zone, roll_basics, ImageFacts, ImagePins, RollMachines,
};
use crate::plan::{Place, SetupNetwork};

fn refused(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Refused {
        reason: reason.into(),
    })
}

/// The Compute Engine calls a roll makes. Paths are relative to
/// `https://compute.googleapis.com/compute/v1/`.
#[async_trait]
pub trait GcpMachineApi: Send + Sync {
    /// GET `path`, `None` when it doesn't exist.
    async fn get(&self, path: &str) -> Result<Option<Value>>;
    /// Names of the instance templates in the project whose name matches
    /// the RE2 expression `pattern`.
    async fn template_names(&self, pattern: &str) -> Result<Vec<String>>;
    /// POST `body` to `path` and wait for the operation to finish.
    async fn insert(&self, path: &str, body: &Value) -> Result<()>;
    /// PATCH `path` with `body` and wait for the operation to finish.
    async fn patch(&self, path: &str, body: &Value) -> Result<()>;
    /// The instance URLs of MIG `path`'s members.
    async fn members(&self, path: &str) -> Result<Vec<String>>;
}

/// `name` is lower case letters, digits and dashes.
fn is_gcp_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// The project in an image path `projects/<project>/global/images/<name>`.
pub fn image_project(image: &str) -> Result<&str> {
    match image.split('/').collect::<Vec<_>>().as_slice() {
        ["projects", project, "global", "images", name]
            if is_gcp_name(project) && is_gcp_name(name) =>
        {
            Ok(project)
        }
        _ => Err(refused(format!(
            "'{image}' is not projects/<project>/global/images/<name>"
        ))),
    }
}

/// What Compute Engine says about image `path`: its owner is the project it
/// lives in.
pub fn image_facts(path: &str, image: &Value) -> Result<ImageFacts> {
    let owner = image_project(path)?.to_string();
    let missing = |field: &str| {
        AlienError::new(ErrorData::CloudCallFailed {
            operation: format!("compute images.get {path} returned no {field}"),
        })
    };
    let created = image["creationTimestamp"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .ok_or_else(|| missing("creation time"))?;
    let architecture = match image["architecture"].as_str() {
        Some("X86_64") => "x86_64",
        Some("ARM64") => "arm64",
        Some(other) => other,
        None => return Err(missing("architecture")),
    };
    if image["status"].as_str() != Some("READY") {
        return Err(refused(format!("image {path} is not READY")));
    }
    let root_disk_gib = image["diskSizeGb"]
        .as_str()
        .and_then(|size| size.parse().ok())
        .ok_or_else(|| missing("disk size"))?;
    Ok(ImageFacts {
        owner,
        architecture: architecture.to_string(),
        created: created.with_timezone(&Utc),
        root_disk_gib,
    })
}

/// Instance metadata key that marks a machine the verifier launched. Not
/// `block-project-ssh-keys`, which customers also set for hardening.
pub const SIGNED_MACHINE_KEY: &str = "alien-signed";

/// The metadata key a machine reads instance tag `tag` from: the tag's
/// words, lower case, joined by dashes, as the machine agent derives it.
pub fn metadata_key(tag: &str) -> String {
    let mut key = String::new();
    for (index, character) in tag.chars().enumerate() {
        if character.is_ascii_uppercase() && index > 0 {
            key.push('-');
        }
        key.push(character.to_ascii_lowercase());
    }
    key
}

/// The setup-fixed subnetwork (`projects/<p>/regions/<r>/subnetworks/<s>`),
/// network tags and service account machines run with.
fn machine_network(place: &Place) -> Result<(&str, &[String], &str)> {
    let network: &SetupNetwork = place.machine_network.as_ref().ok_or_else(|| {
        refused("setup fixed no machine network for this verifier, so it launches no machines")
    })?;
    let invalid = |what: &str| {
        AlienError::new(ErrorData::ConfigurationInvalid {
            message: format!("the machine network setup fixed has {what}"),
        })
    };
    let [subnetwork] = network.subnet_ids.as_slice() else {
        return Err(invalid("not exactly one subnetwork"));
    };
    if !subnetwork.starts_with("projects/") || !subnetwork.contains("/subnetworks/") {
        return Err(invalid("a subnetwork that is not a subnetwork path"));
    }
    if !network
        .security_group_ids
        .iter()
        .all(|tag| is_gcp_name(tag))
    {
        return Err(invalid("a network tag that is not a GCP name"));
    }
    let identity = network
        .identity
        .as_deref()
        .filter(|email| email.ends_with(".iam.gserviceaccount.com"))
        .ok_or_else(|| invalid("no service account"))?;
    Ok((subnetwork, &network.security_group_ids, identity))
}

/// The instance template `name` machines of the roll boot from.
pub fn template_body(
    place: &Place,
    basics: &crate::machines::RollBasics,
    image: &str,
    name: &str,
) -> Result<Value> {
    let (subnetwork, network_tags, service_account) = machine_network(place)?;
    let mut disks = vec![json!({
        "boot": true,
        "autoDelete": true,
        "initializeParams": {
            "sourceImage": image,
            "diskSizeGb": basics.root_disk_gib.to_string(),
            "diskType": "pd-balanced",
        },
    })];
    if let Some(size) = basics.group.data_disk_size_gib() {
        disks.push(json!({
            "autoDelete": true,
            "initializeParams": {"diskSizeGb": size.to_string(), "diskType": "pd-balanced"},
        }));
    }
    let mut metadata: Vec<Value> = basics
        .tags
        .iter()
        .map(|(tag, value)| json!({"key": metadata_key(tag), "value": value}))
        .collect();
    metadata.push(json!({"key": "block-project-ssh-keys", "value": "true"}));
    // Only the verifier writes it: a machine carrying it was launched from a signed release, so
    // the machine agent never runs it unprotected. Management may not set instance or template
    // metadata on a protected install.
    metadata.push(json!({"key": SIGNED_MACHINE_KEY, "value": "true"}));
    Ok(json!({
        "name": name,
        "properties": {
            "machineType": basics.instance_type,
            "disks": disks,
            "networkInterfaces": [{"subnetwork": subnetwork}],
            "tags": {"items": network_tags},
            "serviceAccounts": [{
                "email": service_account,
                "scopes": ["https://www.googleapis.com/auth/cloud-platform"],
            }],
            "metadata": {"items": metadata},
            "shieldedInstanceConfig": {"enableVtpm": true, "enableIntegrityMonitoring": true},
            "advancedMachineFeatures": {
                "enableNestedVirtualization": basics.group.nested_virtualization == Some(true),
            },
        },
    }))
}

/// Roll the group to the signed release: add the next template and create
/// or point the MIG. Answers `{template, version, group}`.
#[allow(clippy::too_many_arguments)]
pub async fn roll(
    api: &dyn GcpMachineApi,
    place: &Place,
    project: &str,
    region: &str,
    release: &ReleaseBody,
    request: &RollMachines,
    pins: &ImagePins,
) -> Result<Value> {
    image_project(&request.machine_image)?;
    let image = api
        .get(&request.machine_image)
        .await?
        .ok_or_else(|| refused(format!("image {} does not exist", request.machine_image)))?;
    let facts = image_facts(&request.machine_image, &image)?;
    let basics = roll_basics(place, release, request, pins, &facts)?;
    // The caller may name the network it expects; it must be setup's.
    let (subnetwork, network_tags, _) = machine_network(place)?;
    check_setup_ids(
        "subnet",
        request.subnet_ids.as_deref(),
        &[subnetwork.to_string()],
    )?;
    check_setup_ids(
        "network tag",
        request.security_group_ids.as_deref(),
        network_tags,
    )?;
    if let Some(zone) = &request.zone {
        check_zone(zone, region)?;
    }
    let pattern = format!("{}-[0-9]+", basics.name);
    let version = api
        .template_names(&pattern)
        .await?
        .iter()
        .filter_map(|name| {
            name.strip_prefix(&format!("{}-", basics.name))?
                .parse::<u64>()
                .ok()
        })
        .max()
        .unwrap_or(0)
        + 1;
    let template = format!("{}-{version}", basics.name);
    if template.len() > 63 {
        return Err(refused(format!(
            "instance template name '{template}' is longer than GCP allows"
        )));
    }
    let body = template_body(place, &basics, &request.machine_image, &template)?;
    api.insert(
        &format!("projects/{project}/global/instanceTemplates"),
        &body,
    )
    .await?;
    let template_link = format!("projects/{project}/global/instanceTemplates/{template}");
    let scope = match &request.zone {
        Some(zone) => format!("projects/{project}/zones/{zone}"),
        None => format!("projects/{project}/regions/{region}"),
    };
    let group_path = format!("{scope}/instanceGroupManagers/{}", basics.name);
    let update_policy = json!({"type": "OPPORTUNISTIC"});
    match api.get(&group_path).await? {
        None => {
            api.insert(
                &format!("{scope}/instanceGroupManagers"),
                &json!({
                    "name": basics.name,
                    "baseInstanceName": basics.name,
                    "instanceTemplate": template_link,
                    "targetSize": basics.group.min_size,
                    "updatePolicy": update_policy,
                }),
            )
            .await?
        }
        Some(_) => {
            api.patch(
                &group_path,
                &json!({
                    "instanceTemplate": template_link,
                    "versions": [{"instanceTemplate": template_link}],
                    "updatePolicy": update_policy,
                }),
            )
            .await?
        }
    }
    Ok(json!({"template": template, "version": version, "group": basics.name}))
}

/// Delete `machines` (instance names) from MIG `name`, zonal in `zone`,
/// keeping at least `minimum`. Answers the removed names and the new size.
#[allow(clippy::too_many_arguments)]
pub async fn remove(
    api: &dyn GcpMachineApi,
    project: &str,
    region: &str,
    name: &str,
    zone: Option<&str>,
    machines: &[String],
    minimum: u32,
) -> Result<(Vec<String>, u32)> {
    let scope = match zone {
        Some(zone) => {
            check_zone(zone, region)?;
            format!("projects/{project}/zones/{zone}")
        }
        None => format!("projects/{project}/regions/{region}"),
    };
    let path = format!("{scope}/instanceGroupManagers/{name}");
    let group = api
        .get(&path)
        .await?
        .ok_or_else(|| refused(format!("group {name} does not exist")))?;
    let size = group["targetSize"]
        .as_u64()
        .and_then(|size| u32::try_from(size).ok())
        .ok_or_else(|| {
            AlienError::new(ErrorData::CloudCallFailed {
                operation: format!("group {name} reports no target size"),
            })
        })?;
    let members = api.members(&path).await?;
    let mut instances = Vec::new();
    for machine in machines {
        let instance = members
            .iter()
            .find(|url| url.rsplit('/').next() == Some(machine.as_str()))
            .ok_or_else(|| refused(format!("machine {machine} is not a member of group {name}")))?;
        instances.push(instance.clone());
    }
    let remaining = crate::machines::size_after_removal(size, machines.len(), minimum)?;
    api.insert(
        &format!("{path}/deleteInstances"),
        &json!({"instances": instances, "skipInstancesOnValidationError": false}),
    )
    .await?;
    Ok((machines.to_vec(), remaining))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::plan::SetupNetwork;
    use alien_core::release_statement::{release_body, ReleaseOptions};
    use alien_core::{CapacityGroup, ComputeCluster, Platform, ResourceLifecycle, Stack};
    use alien_signing::InstallId;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    const PUBLISHER: &str = "machine-images";
    const IMAGE: &str = "projects/machine-images/global/images/machine-x86-20261101";
    const SUBNETWORK: &str =
        "projects/shop-prod/regions/us-central1/subnetworks/acme-prod-machines";
    const SERVICE_ACCOUNT: &str = "acme-prod-compute-general@shop-prod.iam.gserviceaccount.com";

    fn place() -> Place {
        Place {
            install: InstallId::Gcp {
                project: "shop-prod".to_string(),
                prefix: PREFIX.to_string(),
            },
            prefix: PREFIX.to_string(),
            vpc: None,
            machine_network: Some(SetupNetwork {
                subnet_ids: vec![SUBNETWORK.to_string()],
                security_group_ids: vec!["acme-prod-machines".to_string()],
                identity: Some(SERVICE_ACCOUNT.to_string()),
            }),
        }
    }

    fn release() -> ReleaseBody {
        let stack = Stack::new("shop".to_string())
            .add(
                ComputeCluster::new("compute".to_string())
                    .capacity_group(CapacityGroup {
                        group_id: "general".to_string(),
                        instance_type: Some("n2-standard-4".to_string()),
                        profile: None,
                        min_size: 1,
                        max_size: 3,
                        scale_policy: None,
                        nested_virtualization: None,
                        data_disk_gib_range: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        release_body(
            Platform::Gcp,
            &stack,
            &ReleaseOptions {
                channel: CHANNEL.to_string(),
                components: BTreeMap::from([(
                    "machine-agent/linux-x86_64".to_string(),
                    format!("sha256:{}", "2".repeat(64)),
                )]),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn request() -> RollMachines {
        RollMachines {
            cluster: "compute".to_string(),
            group: "general".to_string(),
            release: signed_release(1, &release()),
            signer_lists: vec![signer_list()],
            config: None,
            machine_image: IMAGE.to_string(),
            instance_type: None,
            api_url: None,
            security_group_ids: None,
            subnet_ids: None,
            zone: Some("us-central1-a".to_string()),
        }
    }

    fn pins() -> ImagePins {
        ImagePins {
            publisher: Some(PUBLISHER.to_string()),
            minimum_created: Some("2026-08-01T00:00:00Z".parse().unwrap()),
        }
    }

    /// An image as images.get returns it (fields the verifier reads).
    fn image(created: &str) -> Value {
        json!({"name": "machine-x86-20261101", "status": "READY", "architecture": "X86_64",
               "creationTimestamp": created, "diskSizeGb": "40"})
    }

    /// Records each call the way Compute Engine receives it.
    #[derive(Default)]
    struct Recorded {
        images: BTreeMap<String, Value>,
        templates: Vec<String>,
        group: Option<Value>,
        calls: Mutex<Vec<(String, String, Value)>>,
    }

    #[async_trait]
    impl GcpMachineApi for Recorded {
        async fn get(&self, path: &str) -> Result<Option<Value>> {
            if path.contains("/instanceGroupManagers/") {
                return Ok(self.group.clone());
            }
            Ok(self.images.get(path).cloned())
        }
        async fn template_names(&self, pattern: &str) -> Result<Vec<String>> {
            assert_eq!(pattern, "acme-prod-compute-general-[0-9]+");
            Ok(self.templates.clone())
        }
        async fn insert(&self, path: &str, body: &Value) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(("POST".into(), path.into(), body.clone()));
            Ok(())
        }
        async fn patch(&self, path: &str, body: &Value) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(("PATCH".into(), path.into(), body.clone()));
            Ok(())
        }
        async fn members(&self, path: &str) -> Result<Vec<String>> {
            assert!(path.ends_with("/instanceGroupManagers/acme-prod-compute-general"));
            Ok(vec![
                "https://www.googleapis.com/compute/v1/projects/shop-prod/zones/us-central1-a/instances/acme-prod-compute-general-a1b2".into(),
                "https://www.googleapis.com/compute/v1/projects/shop-prod/zones/us-central1-a/instances/acme-prod-compute-general-c3d4".into(),
            ])
        }
    }

    fn api(created: &str) -> Recorded {
        Recorded {
            images: BTreeMap::from([(IMAGE.to_string(), image(created))]),
            ..Default::default()
        }
    }

    async fn roll_with(api: &Recorded, request: &RollMachines) -> Result<Value> {
        roll(
            api,
            &place(),
            "shop-prod",
            "us-central1",
            &release(),
            request,
            &pins(),
        )
        .await
    }

    #[tokio::test]
    async fn a_first_roll_writes_template_1_and_creates_the_group() {
        let api = api("2026-11-01T10:00:00.000-07:00");
        let answer = roll_with(&api, &request()).await.unwrap();
        assert_eq!(
            answer,
            json!({"template": "acme-prod-compute-general-1", "version": 1, "group": "acme-prod-compute-general"})
        );
        let calls = api.calls.into_inner().unwrap();
        assert_eq!(calls.len(), 2, "{calls:?}");
        let (method, path, template) = &calls[0];
        assert_eq!(
            (method.as_str(), path.as_str()),
            ("POST", "projects/shop-prod/global/instanceTemplates")
        );
        let properties = &template["properties"];
        assert_eq!(properties["machineType"], "n2-standard-4");
        assert_eq!(
            properties["disks"][0]["initializeParams"]["sourceImage"],
            IMAGE
        );
        // The image's 40 GB, not the signed group's smaller root disk.
        assert_eq!(
            properties["disks"][0]["initializeParams"]["diskSizeGb"],
            "40"
        );
        assert_eq!(
            properties["networkInterfaces"],
            json!([{"subnetwork": SUBNETWORK}])
        );
        assert_eq!(properties["serviceAccounts"][0]["email"], SERVICE_ACCOUNT);
        assert_eq!(properties["tags"]["items"], json!(["acme-prod-machines"]));
        assert_eq!(
            properties["metadata"]["items"],
            json!([
                {"key": metadata_key(crate::machines::CAPACITY_GROUP_TAG), "value": "general"},
                {"key": "block-project-ssh-keys", "value": "true"},
                // Marks a machine the verifier launched; customers set block-project-ssh-keys
                // for hardening on unprotected installs too, so it can't be the marker.
                {"key": "alien-signed", "value": "true"},
            ])
        );
        assert!(
            properties.get("networkInterfaces").unwrap()[0]
                .get("accessConfigs")
                .is_none(),
            "no public address"
        );
        let (method, path, group) = &calls[1];
        assert_eq!(
            (method.as_str(), path.as_str()),
            (
                "POST",
                "projects/shop-prod/zones/us-central1-a/instanceGroupManagers"
            )
        );
        assert_eq!(group["targetSize"], 1);
        assert_eq!(
            group["instanceTemplate"],
            "projects/shop-prod/global/instanceTemplates/acme-prod-compute-general-1"
        );
        assert_eq!(group["updatePolicy"], json!({"type": "OPPORTUNISTIC"}));
    }

    #[tokio::test]
    async fn a_next_roll_adds_the_next_template_and_points_the_group() {
        let mut api = api("2026-11-01T10:00:00Z");
        api.templates = vec![
            "acme-prod-compute-general-1".into(),
            "acme-prod-compute-general-7".into(),
        ];
        api.group = Some(json!({"name": "acme-prod-compute-general"}));
        let mut regional = request();
        regional.zone = None;
        let answer = roll_with(&api, &regional).await.unwrap();
        assert_eq!(answer["version"], 8);
        let calls = api.calls.into_inner().unwrap();
        assert_eq!(calls[1].0, "PATCH");
        assert_eq!(calls[1].1, "projects/shop-prod/regions/us-central1/instanceGroupManagers/acme-prod-compute-general");
        assert_eq!(
            calls[1].2["versions"],
            json!([{"instanceTemplate": "projects/shop-prod/global/instanceTemplates/acme-prod-compute-general-8"}])
        );
    }

    #[tokio::test]
    async fn foreign_old_or_misnamed_images_and_zones_write_nothing() {
        let cases: Vec<(RollMachines, Recorded, &str)> = vec![
            (
                RollMachines {
                    machine_image: "projects/ubuntu-os-cloud/global/images/ubuntu-2204".into(),
                    ..request()
                },
                Recorded {
                    images: BTreeMap::from([(
                        "projects/ubuntu-os-cloud/global/images/ubuntu-2204".to_string(),
                        image("2026-11-01T00:00:00Z"),
                    )]),
                    ..Default::default()
                },
                "is owned by ubuntu-os-cloud, not the publisher machine-images",
            ),
            (
                request(),
                api("2026-01-01T00:00:00Z"),
                "the oldest image this build boots",
            ),
            (
                RollMachines {
                    machine_image: "https://evil.example/projects/machine-images/global/images/x"
                        .into(),
                    ..request()
                },
                api("2026-11-01T00:00:00Z"),
                "is not projects/<project>/global/images/<name>",
            ),
            (
                RollMachines {
                    zone: Some("europe-west1-b".into()),
                    ..request()
                },
                api("2026-11-01T00:00:00Z"),
                "is not a zone of the verifier's region us-central1",
            ),
            (
                RollMachines {
                    subnet_ids: Some(vec![
                        "projects/shop-prod/regions/us-central1/subnetworks/default".into(),
                    ]),
                    ..request()
                },
                api("2026-11-01T00:00:00Z"),
                "subnet projects/shop-prod/regions/us-central1/subnetworks/default is not one setup created",
            ),
            (
                RollMachines {
                    security_group_ids: Some(vec!["allow-all".into()]),
                    ..request()
                },
                api("2026-11-01T00:00:00Z"),
                "network tag allow-all is not one setup created",
            ),
            (
                RollMachines {
                    instance_type: Some("a3-highgpu-8g".into()),
                    ..request()
                },
                api("2026-11-01T00:00:00Z"),
                "is not the signed n2-standard-4",
            ),
        ];
        for (request, api, reason) in cases {
            let error = crate::describe(&roll_with(&api, &request).await.unwrap_err());
            assert!(error.contains(reason), "{reason}: {error}");
            assert!(
                api.calls.into_inner().unwrap().is_empty(),
                "{reason}: nothing written"
            );
        }
    }

    #[tokio::test]
    async fn removal_deletes_only_members_and_keeps_the_signed_minimum() {
        let removal = |machines: &[&str], size: u64| {
            let api = Recorded {
                group: Some(json!({"targetSize": size})),
                ..Default::default()
            };
            let machines: Vec<String> = machines.iter().map(|m| m.to_string()).collect();
            async move {
                let result = remove(
                    &api,
                    "shop-prod",
                    "us-central1",
                    "acme-prod-compute-general",
                    Some("us-central1-a"),
                    &machines,
                    1,
                )
                .await;
                (result, api.calls.into_inner().unwrap())
            }
        };
        let (result, calls) = removal(&["acme-prod-compute-general-a1b2"], 2).await;
        assert_eq!(
            result.unwrap(),
            (vec!["acme-prod-compute-general-a1b2".to_string()], 1)
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, "projects/shop-prod/zones/us-central1-a/instanceGroupManagers/acme-prod-compute-general/deleteInstances");
        assert_eq!(calls[0].2["skipInstancesOnValidationError"], false);
        assert_eq!(calls[0].2["instances"].as_array().unwrap().len(), 1);

        // Attack: a machine of another group.
        let (result, calls) = removal(&["acme-prod-compute-big-zz99"], 2).await;
        assert!(crate::describe(&result.unwrap_err())
            .contains("is not a member of group acme-prod-compute-general"));
        assert!(calls.is_empty());
        // Attack: scale below the signed minimum by deleting.
        let (result, calls) = removal(
            &[
                "acme-prod-compute-general-a1b2",
                "acme-prod-compute-general-c3d4",
            ],
            2,
        )
        .await;
        assert!(crate::describe(&result.unwrap_err()).contains("below the signed minimum 1"));
        assert!(calls.is_empty());
    }

    #[test]
    fn metadata_keys_are_the_tags_in_kebab_case() {
        assert_eq!(metadata_key("CapacityGroup"), "capacity-group");
        assert_eq!(metadata_key("ApiUrl"), "api-url");
    }
}
