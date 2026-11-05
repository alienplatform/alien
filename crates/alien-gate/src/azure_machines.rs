//! Machines on protected Azure installs: the verifier writes the VMSS model.
//!
//! The scale set `<prefix>-<cluster>-<group>` in the install's resource
//! group is created at the signed minimum, or its model replaced, with a
//! manual upgrade policy: machines already running keep their model until
//! the platform replaces them, as on AWS. Machines boot the publisher's
//! community-gallery image with setup's subnet, network security group and
//! identity, and read their capacity group from the scale set's tags. The
//! admin account takes only an SSH key whose private half the verifier
//! never keeps.

use async_trait::async_trait;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

use alien_error::AlienError;
use alien_signing::ReleaseBody;

use crate::error::{ErrorData, Result};
use crate::machines::{roll_basics, ImageFacts, ImagePins, RollBasics, RollMachines};
use crate::plan::Place;

/// API version of the scale set calls.
pub const VMSS_API: &str = "2024-07-01";
/// API version of the community gallery reads.
pub const GALLERY_API: &str = "2023-07-03";

fn refused(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Refused {
        reason: reason.into(),
    })
}

/// The ARM calls a roll makes. Paths start with `/subscriptions/` and
/// carry their `api-version`.
#[async_trait]
pub trait AzureMachineApi: Send + Sync {
    /// GET `path`, `None` when it doesn't exist.
    async fn get(&self, path: &str) -> Result<Option<Value>>;
    /// PUT `body` to `path` and wait until it is provisioned.
    async fn put(&self, path: &str, body: &Value) -> Result<Value>;
}

fn is_azure_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

/// The parts of `/CommunityGalleries/<gallery>/Images/<image>/Versions/<version>`.
pub fn image_parts(image: &str) -> Result<(&str, &str, &str)> {
    match image.split('/').collect::<Vec<_>>().as_slice() {
        ["", "CommunityGalleries", gallery, "Images", definition, "Versions", version]
            if is_azure_name(gallery) && is_azure_name(definition) && is_azure_name(version) =>
        {
            Ok((gallery, definition, version))
        }
        _ => Err(refused(format!(
            "'{image}' is not /CommunityGalleries/<gallery>/Images/<image>/Versions/<version>"
        ))),
    }
}

/// What the gallery says about the image: the gallery is its publisher, the
/// definition names its architecture and the version when it was published.
pub fn image_facts(image: &str, definition: &Value, version: &Value) -> Result<ImageFacts> {
    let (gallery, _, _) = image_parts(image)?;
    let missing = |field: &str| {
        AlienError::new(ErrorData::CloudCallFailed {
            operation: format!("community gallery read of {image} returned no {field}"),
        })
    };
    let architecture = match definition["properties"]["architecture"].as_str() {
        Some("x64") => "x86_64",
        Some("Arm64") => "arm64",
        Some(other) => other,
        None => return Err(missing("architecture")),
    };
    let created = version["properties"]["publishingProfile"]["publishedDate"]
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        .ok_or_else(|| missing("published date"))?;
    Ok(ImageFacts {
        owner: gallery.to_string(),
        architecture: architecture.to_string(),
        created: created.with_timezone(&Utc),
    })
}

/// An OpenSSH `ssh-ed25519` public key for a key pair made here and
/// dropped: Azure needs an admin credential, and nobody may hold one.
pub fn unusable_ssh_key() -> Result<String> {
    let mut seed = [0u8; 32];
    rand::fill(&mut seed);
    let pair = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new(seed));
    let mut blob = Vec::new();
    for part in [b"ssh-ed25519".as_slice(), pair.pk.as_slice()] {
        blob.extend_from_slice(&(part.len() as u32).to_be_bytes());
        blob.extend_from_slice(part);
    }
    Ok(format!(
        "ssh-ed25519 {}",
        base64::engine::general_purpose::STANDARD.encode(blob)
    ))
}

/// Setup's subnet, network security group (if any) and identity.
fn machine_network(place: &Place) -> Result<(&str, Option<&str>, &str)> {
    let network = place.machine_network.as_ref().ok_or_else(|| {
        refused("setup fixed no machine network for this verifier, so it launches no machines")
    })?;
    let invalid = |what: &str| {
        AlienError::new(ErrorData::ConfigurationInvalid {
            message: format!("the machine network setup fixed has {what}"),
        })
    };
    let [subnet] = network.subnet_ids.as_slice() else {
        return Err(invalid("not exactly one subnet"));
    };
    if !subnet.starts_with("/subscriptions/") || !subnet.contains("/subnets/") {
        return Err(invalid("a subnet that is not a subnet id"));
    }
    let security_group = match network.security_group_ids.as_slice() {
        [] => None,
        [group] if group.contains("/networkSecurityGroups/") => Some(group.as_str()),
        _ => {
            return Err(invalid(
                "more than one network security group, or not an id",
            ))
        }
    };
    let identity = network
        .identity
        .as_deref()
        .filter(|id| id.contains("/userAssignedIdentities/"))
        .ok_or_else(|| invalid("no user-assigned identity"))?;
    Ok((subnet, security_group, identity))
}

/// The scale set as the verifier writes it.
pub fn scale_set_body(
    place: &Place,
    basics: &RollBasics,
    image: &str,
    location: &str,
    capacity: u32,
    ssh_key: &str,
) -> Result<Value> {
    let (subnet, security_group, identity) = machine_network(place)?;
    let mut interface = json!({
        "primary": true,
        "ipConfigurations": [{"name": "primary", "properties": {"subnet": {"id": subnet}}}],
    });
    if let Some(group) = security_group {
        interface["networkSecurityGroup"] = json!({"id": group});
    }
    let mut storage = json!({
        "imageReference": {"communityGalleryImageId": image},
        "osDisk": {
            "createOption": "FromImage",
            "caching": "ReadWrite",
            "managedDisk": {"storageAccountType": "StandardSSD_LRS"},
            "diskSizeGB": basics.group.aws_root_volume_size_gib(),
        },
    });
    if let Some(size) = basics.group.data_disk_size_gib() {
        storage["dataDisks"] = json!([{"lun": 0, "createOption": "Empty", "diskSizeGB": size}]);
    }
    let tags: serde_json::Map<String, Value> = basics
        .tags
        .iter()
        .map(|(key, value)| (key.clone(), json!(value)))
        .collect();
    let prefix: String = basics.name.chars().take(15).collect();
    Ok(json!({
        "location": location,
        "tags": tags,
        "sku": {"name": basics.instance_type, "tier": "Standard", "capacity": capacity},
        "identity": {"type": "UserAssigned", "userAssignedIdentities": {identity: {}}},
        "properties": {
            "upgradePolicy": {"mode": "Manual"},
            "overprovision": false,
            "virtualMachineProfile": {
                "storageProfile": storage,
                "osProfile": {
                    "computerNamePrefix": prefix.trim_end_matches('-'),
                    "adminUsername": "machine",
                    "linuxConfiguration": {
                        "disablePasswordAuthentication": true,
                        "ssh": {"publicKeys": [{
                            "path": "/home/machine/.ssh/authorized_keys",
                            "keyData": ssh_key,
                        }]},
                    },
                },
                "networkProfile": {"networkInterfaceConfigurations": [
                    {"name": "primary", "properties": interface},
                ]},
            },
        },
    }))
}

/// Roll the group to the signed release: write the scale set model, at the
/// signed minimum when new, keeping its capacity within the signed bounds
/// otherwise. Answers `{template, version, group}`: the scale set, its
/// model's etag, and the scale set again.
pub async fn roll(
    api: &dyn AzureMachineApi,
    place: &Place,
    subscription: &str,
    resource_group: &str,
    release: &ReleaseBody,
    request: &RollMachines,
    pins: &ImagePins,
) -> Result<Value> {
    if request.zone.is_some() {
        return Err(refused("a scale set takes no zone"));
    }
    let (gallery, definition, version) = image_parts(&request.machine_image)?;
    let group_path = format!("/subscriptions/{subscription}/resourceGroups/{resource_group}");
    let location = api
        .get(&format!("{group_path}?api-version=2021-04-01"))
        .await?
        .and_then(|group| group["location"].as_str().map(str::to_string))
        .ok_or_else(|| refused(format!("resource group {resource_group} has no location")))?;
    let gallery_path = format!(
        "/subscriptions/{subscription}/providers/Microsoft.Compute/locations/{location}/communityGalleries/{gallery}/images/{definition}"
    );
    let not_found = || refused(format!("image {} does not exist", request.machine_image));
    let definition_body = api
        .get(&format!("{gallery_path}?api-version={GALLERY_API}"))
        .await?
        .ok_or_else(not_found)?;
    let version_body = api
        .get(&format!(
            "{gallery_path}/versions/{version}?api-version={GALLERY_API}"
        ))
        .await?
        .ok_or_else(not_found)?;
    let facts = image_facts(&request.machine_image, &definition_body, &version_body)?;
    let basics = roll_basics(place, release, request, pins, &facts)?;
    let set_path = format!(
        "{group_path}/providers/Microsoft.Compute/virtualMachineScaleSets/{}?api-version={VMSS_API}",
        basics.name
    );
    let capacity = match api.get(&set_path).await? {
        None => basics.group.min_size,
        Some(existing) => crate::machines::clamp(
            existing["sku"]["capacity"]
                .as_u64()
                .and_then(|capacity| u32::try_from(capacity).ok())
                .ok_or_else(|| {
                    AlienError::new(ErrorData::CloudCallFailed {
                        operation: format!("scale set {} reports no capacity", basics.name),
                    })
                })?,
            &basics.group,
        ),
    };
    let body = scale_set_body(
        place,
        &basics,
        &request.machine_image,
        &location,
        capacity,
        &unusable_ssh_key()?,
    )?;
    let written = api.put(&set_path, &body).await?;
    Ok(json!({
        "template": basics.name,
        "version": written["etag"],
        "group": basics.name,
    }))
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

    const GALLERY: &str = "machines-1111aaaa-2222-3333-4444-555566667777";
    const SUBSCRIPTION: &str = "00000000-0000-0000-0000-000000000000";
    const SUBNET: &str = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/acme-prod/providers/Microsoft.Network/virtualNetworks/acme-prod/subnets/machines";
    const IDENTITY: &str = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/acme-prod/providers/Microsoft.ManagedIdentity/userAssignedIdentities/acme-prod-compute-general";

    fn image() -> String {
        format!("/CommunityGalleries/{GALLERY}/Images/machine-x64/Versions/2026.11.01")
    }

    fn place() -> Place {
        Place {
            install: InstallId::Azure {
                subscription: SUBSCRIPTION.to_string(),
                resource_group: "acme-prod".to_string(),
            },
            prefix: PREFIX.to_string(),
            vpc: None,
            machine_network: Some(SetupNetwork {
                subnet_ids: vec![SUBNET.to_string()],
                security_group_ids: Vec::new(),
                identity: Some(IDENTITY.to_string()),
            }),
        }
    }

    fn release() -> ReleaseBody {
        let stack = Stack::new("shop".to_string())
            .add(
                ComputeCluster::new("compute".to_string())
                    .capacity_group(CapacityGroup {
                        group_id: "general".to_string(),
                        instance_type: Some("Standard_D2s_v5".to_string()),
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
            Platform::Azure,
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
            machine_image: image(),
            instance_type: None,
            api_url: None,
            security_group_ids: None,
            subnet_ids: None,
            zone: None,
        }
    }

    fn pins() -> ImagePins {
        ImagePins {
            publisher: Some(GALLERY.to_string()),
            minimum_created: Some("2026-08-01T00:00:00Z".parse().unwrap()),
        }
    }

    #[derive(Default)]
    struct Recorded {
        published: String,
        existing: Option<Value>,
        puts: Mutex<Vec<(String, Value)>>,
    }

    #[async_trait]
    impl AzureMachineApi for Recorded {
        async fn get(&self, path: &str) -> Result<Option<Value>> {
            Ok(if path.contains("/virtualMachineScaleSets/") {
                self.existing.clone()
            } else if path.contains("/versions/") {
                Some(
                    json!({"properties": {"publishingProfile": {"publishedDate": self.published}}}),
                )
            } else if path.contains("/communityGalleries/") {
                Some(json!({"properties": {"architecture": "x64", "osType": "Linux"}}))
            } else {
                Some(json!({"location": "westeurope"}))
            })
        }
        async fn put(&self, path: &str, body: &Value) -> Result<Value> {
            self.puts
                .lock()
                .unwrap()
                .push((path.to_string(), body.clone()));
            Ok(json!({"etag": "\"3\""}))
        }
    }

    async fn roll_with(api: &Recorded, request: &RollMachines) -> Result<Value> {
        roll(
            api,
            &place(),
            SUBSCRIPTION,
            "acme-prod",
            &release(),
            request,
            &pins(),
        )
        .await
    }

    #[tokio::test]
    async fn a_roll_writes_the_scale_set_model_from_the_signed_group() {
        let api = Recorded {
            published: "2026-11-01T08:00:00.1234567+00:00".to_string(),
            ..Default::default()
        };
        let answer = roll_with(&api, &request()).await.unwrap();
        assert_eq!(
            answer,
            json!({"template": "acme-prod-compute-general", "version": "\"3\"", "group": "acme-prod-compute-general"})
        );
        let puts = api.puts.into_inner().unwrap();
        let [(path, body)] = puts.as_slice() else {
            panic!("{puts:?}")
        };
        assert_eq!(
            path,
            &format!("/subscriptions/{SUBSCRIPTION}/resourceGroups/acme-prod/providers/Microsoft.Compute/virtualMachineScaleSets/acme-prod-compute-general?api-version={VMSS_API}")
        );
        assert_eq!(
            body["sku"],
            json!({"name": "Standard_D2s_v5", "tier": "Standard", "capacity": 1})
        );
        assert_eq!(
            body["properties"]["upgradePolicy"],
            json!({"mode": "Manual"})
        );
        let profile = &body["properties"]["virtualMachineProfile"];
        assert_eq!(
            profile["storageProfile"]["imageReference"],
            json!({"communityGalleryImageId": image()})
        );
        assert_eq!(
            profile["networkProfile"]["networkInterfaceConfigurations"][0]["properties"]
                ["ipConfigurations"][0]["properties"]["subnet"]["id"],
            SUBNET
        );
        assert!(
            profile["networkProfile"]["networkInterfaceConfigurations"][0]["properties"]
                ["ipConfigurations"][0]["properties"]
                .get("publicIPAddressConfiguration")
                .is_none()
        );
        assert_eq!(
            body["identity"]["userAssignedIdentities"],
            json!({IDENTITY: {}})
        );
        assert_eq!(
            body["tags"],
            json!({crate::machines::CAPACITY_GROUP_TAG: "general"})
        );
        let linux = &profile["osProfile"]["linuxConfiguration"];
        assert_eq!(linux["disablePasswordAuthentication"], true);
        assert!(linux["ssh"]["publicKeys"][0]["keyData"]
            .as_str()
            .unwrap()
            .starts_with("ssh-ed25519 "));
        assert!(
            profile["osProfile"].get("customData").is_none() && profile.get("userData").is_none()
        );
    }

    #[tokio::test]
    async fn an_existing_set_keeps_its_capacity_within_the_signed_bounds() {
        let api = Recorded {
            published: "2026-11-01T08:00:00Z".to_string(),
            existing: Some(json!({"sku": {"capacity": 9}})),
            ..Default::default()
        };
        roll_with(&api, &request()).await.unwrap();
        assert_eq!(api.puts.into_inner().unwrap()[0].1["sku"]["capacity"], 3);
    }

    #[tokio::test]
    async fn foreign_or_old_images_write_nothing() {
        for (image, published, reason) in [
            ("/CommunityGalleries/other-gallery/Images/machine-x64/Versions/1.0.0".to_string(), "2026-11-01T00:00:00Z", "is owned by other-gallery, not the publisher"),
            (image(), "2026-01-01T00:00:00Z", "the oldest image this build boots"),
            ("/subscriptions/x/resourceGroups/y/providers/Microsoft.Compute/galleries/z/images/w/versions/1".to_string(), "2026-11-01T00:00:00Z", "is not /CommunityGalleries/"),
        ] {
            let api = Recorded { published: published.to_string(), ..Default::default() };
            let mut request = request();
            request.machine_image = image;
            let error = crate::describe(&roll_with(&api, &request).await.unwrap_err());
            assert!(error.contains(reason), "{reason}: {error}");
            assert!(api.puts.into_inner().unwrap().is_empty());
        }
    }

    #[test]
    fn the_ssh_key_is_an_openssh_ed25519_key() {
        let key = unusable_ssh_key().unwrap();
        let blob = base64::engine::general_purpose::STANDARD
            .decode(key.strip_prefix("ssh-ed25519 ").unwrap())
            .unwrap();
        assert_eq!(&blob[..4], &11u32.to_be_bytes());
        assert_eq!(&blob[4..15], b"ssh-ed25519");
        assert_eq!(&blob[15..19], &32u32.to_be_bytes());
        assert_eq!(blob.len(), 19 + 32);
        assert_ne!(key, unusable_ssh_key().unwrap());
    }
}
