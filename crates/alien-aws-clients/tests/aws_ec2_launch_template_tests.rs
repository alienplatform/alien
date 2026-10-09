//! Live check that a launch template version created from a source version launches
//! machines with the overrides it was given.
//!
//! With `SourceVersion`, EC2 copies every launch parameter the request omits from the
//! source version. A field the client fails to put on the wire is therefore not an
//! error: the new version silently keeps the old value. This test creates version 1
//! with instance profile A and an 8 GiB root volume, creates version 2 from it with
//! instance profile B, a 12 GiB root volume and a different instance type, then lets an
//! Auto Scaling group launch a machine from `$Latest` and inspects the machine EC2
//! actually started.
//!
//! Runs against the AWS account configured in `.env.test` (`AWS_TARGET_*`), in its
//! default VPC. Every resource is named with a random prefix and deleted by exact ID.

use std::path::PathBuf as StdPathBuf;
use std::time::Duration;

use alien_aws_clients::autoscaling::{
    AutoScalingApi, AutoScalingClient, CreateAutoScalingGroupRequest,
    DeleteAutoScalingGroupRequest, DescribeAutoScalingGroupsRequest, LaunchTemplateSpecification,
};
use alien_aws_clients::ec2::*;
use alien_aws_clients::iam::{CreateInstanceProfileRequest, CreateRoleRequest};
use alien_aws_clients::{
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, IamApi, IamClient,
};
use reqwest::Client;
use test_context::{test_context, AsyncTestContext};
use tracing::info;
use uuid::Uuid;

const EC2_TRUST_POLICY: &str = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":{"Service":"ec2.amazonaws.com"},"Action":"sts:AssumeRole"}]}"#;
const AMI: &str =
    "resolve:ssm:/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64";
const ROOT_DEVICE: &str = "/dev/xvda";

struct LaunchTemplateContext {
    ec2: Ec2Client,
    asg: AutoScalingClient,
    iam: IamClient,
    prefix: String,
    instance_profiles: Vec<String>,
    security_group_id: Option<String>,
    launch_template_id: Option<String>,
    asg_name: Option<String>,
    instance_ids: Vec<String>,
}

impl AsyncTestContext for LaunchTemplateContext {
    async fn setup() -> Self {
        let root: StdPathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).ok();
        tracing_subscriber::fmt::try_init().ok();

        let config = AwsClientConfig {
            account_id: std::env::var("AWS_TARGET_ACCOUNT_ID").expect("AWS_TARGET_ACCOUNT_ID"),
            region: std::env::var("AWS_TARGET_REGION").expect("AWS_TARGET_REGION"),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: std::env::var("AWS_TARGET_ACCESS_KEY_ID")
                    .expect("AWS_TARGET_ACCESS_KEY_ID"),
                secret_access_key: std::env::var("AWS_TARGET_SECRET_ACCESS_KEY")
                    .expect("AWS_TARGET_SECRET_ACCESS_KEY"),
                session_token: std::env::var("AWS_TARGET_SESSION_TOKEN").ok(),
            },
            service_overrides: None,
        };
        let credentials = || AwsCredentialProvider::from_config_sync(config.clone());

        Self {
            ec2: Ec2Client::new(Client::new(), credentials()),
            asg: AutoScalingClient::new(Client::new(), credentials()),
            iam: IamClient::new(Client::new(), credentials()),
            prefix: format!(
                "alien-lt-test-{}",
                &Uuid::new_v4().simple().to_string()[..8]
            ),
            instance_profiles: Vec::new(),
            security_group_id: None,
            launch_template_id: None,
            asg_name: None,
            instance_ids: Vec::new(),
        }
    }

    async fn teardown(self) {
        let mut failures = Vec::new();

        if let Some(asg_name) = &self.asg_name {
            if let Err(e) = self
                .asg
                .delete_auto_scaling_group(
                    DeleteAutoScalingGroupRequest::builder()
                        .auto_scaling_group_name(asg_name.clone())
                        .force_delete(true)
                        .build(),
                )
                .await
            {
                failures.push(format!("delete ASG {asg_name}: {e:?}"));
            }
        }
        // The group may have launched machines this test never observed.
        let mut instance_ids = self.instance_ids.clone();
        if let Some(asg_name) = &self.asg_name {
            match self.instances_tagged_with_group(asg_name).await {
                Ok(ids) => instance_ids.extend(ids),
                Err(e) => failures.push(format!("list instances of {asg_name}: {e:?}")),
            }
        }
        instance_ids.sort();
        instance_ids.dedup();
        if !instance_ids.is_empty() {
            if let Err(e) = self.ec2.terminate_instances(instance_ids.clone()).await {
                failures.push(format!("terminate {instance_ids:?}: {e:?}"));
            }
            if let Err(e) = self.wait_until_terminated(&instance_ids).await {
                failures.push(e);
            }
        }
        if let Some(id) = &self.launch_template_id {
            if let Err(e) = self
                .ec2
                .delete_launch_template(
                    DeleteLaunchTemplateRequest::builder()
                        .launch_template_id(id.clone())
                        .build(),
                )
                .await
            {
                failures.push(format!("delete launch template {id}: {e:?}"));
            }
        }
        if let Some(id) = &self.security_group_id {
            if let Err(e) = self.ec2.delete_security_group(id).await {
                failures.push(format!("delete security group {id}: {e:?}"));
            }
        }
        for name in &self.instance_profiles {
            for result in [
                self.iam.remove_role_from_instance_profile(name, name).await,
                self.iam.delete_instance_profile(name).await,
                self.iam.delete_role(name).await,
            ] {
                if let Err(e) = result {
                    failures.push(format!("delete IAM role/profile {name}: {e:?}"));
                }
            }
        }

        assert!(
            failures.is_empty(),
            "cleanup left resources behind (prefix {}): {failures:#?}",
            self.prefix
        );
    }
}

impl LaunchTemplateContext {
    async fn create_instance_profile(&mut self, suffix: &str) -> String {
        let name = format!("{}-{suffix}", self.prefix);
        self.iam
            .create_role(
                CreateRoleRequest::builder()
                    .role_name(name.clone())
                    .assume_role_policy_document(EC2_TRUST_POLICY.to_string())
                    .build(),
            )
            .await
            .expect("create role");
        let profile = self
            .iam
            .create_instance_profile(
                CreateInstanceProfileRequest::builder()
                    .instance_profile_name(name.clone())
                    .build(),
            )
            .await
            .expect("create instance profile")
            .create_instance_profile_result
            .instance_profile;
        self.instance_profiles.push(name.clone());
        self.iam
            .add_role_to_instance_profile(&name, &name)
            .await
            .expect("add role to instance profile");
        profile.arn
    }

    async fn default_vpc(&self) -> (String, Vec<String>) {
        let vpc_id = self
            .ec2
            .describe_vpcs(
                DescribeVpcsRequest::builder()
                    .filters(vec![Filter::builder()
                        .name("is-default".to_string())
                        .values(vec!["true".to_string()])
                        .build()])
                    .build(),
            )
            .await
            .expect("describe default VPC")
            .vpc_set
            .and_then(|set| set.items.into_iter().next())
            .and_then(|vpc| vpc.vpc_id)
            .expect("the target account needs a default VPC");
        let subnet_ids: Vec<String> = self
            .ec2
            .describe_subnets(
                DescribeSubnetsRequest::builder()
                    .filters(vec![Filter::builder()
                        .name("vpc-id".to_string())
                        .values(vec![vpc_id.clone()])
                        .build()])
                    .build(),
            )
            .await
            .expect("describe default subnets")
            .subnet_set
            .map(|set| set.items.into_iter().filter_map(|s| s.subnet_id).collect())
            .unwrap_or_default();
        assert!(
            !subnet_ids.is_empty(),
            "default VPC {vpc_id} has no subnets"
        );
        (vpc_id, subnet_ids)
    }

    async fn instances_tagged_with_group(&self, asg_name: &str) -> Result<Vec<String>, String> {
        let response = self
            .ec2
            .describe_instances(
                DescribeInstancesRequest::builder()
                    .filters(vec![Filter::builder()
                        .name("tag:aws:autoscaling:groupName".to_string())
                        .values(vec![asg_name.to_string()])
                        .build()])
                    .build(),
            )
            .await
            .map_err(|e| format!("{e:?}"))?;
        Ok(instances(response)
            .into_iter()
            .filter_map(|i| i.instance_id)
            .collect())
    }

    async fn wait_until_terminated(&self, instance_ids: &[String]) -> Result<(), String> {
        for _ in 0..60 {
            let response = self
                .ec2
                .describe_instances(
                    DescribeInstancesRequest::builder()
                        .instance_ids(instance_ids.to_vec())
                        .build(),
                )
                .await
                .map_err(|e| format!("describe {instance_ids:?}: {e:?}"))?;
            let all_terminated = instances(response).iter().all(|i| {
                i.instance_state.as_ref().and_then(|s| s.name.as_deref()) == Some("terminated")
            });
            if all_terminated {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        Err(format!(
            "{instance_ids:?} did not terminate within 10 minutes"
        ))
    }

    /// Waits for the group to start a machine and returns it once EC2 reports it.
    async fn wait_for_launched_instance(&mut self, asg_name: &str) -> Instance {
        for _ in 0..60 {
            let group = self
                .asg
                .describe_auto_scaling_groups(
                    DescribeAutoScalingGroupsRequest::builder()
                        .auto_scaling_group_names(vec![asg_name.to_string()])
                        .build(),
                )
                .await
                .expect("describe ASG")
                .describe_auto_scaling_groups_result
                .auto_scaling_groups
                .and_then(|groups| groups.members.into_iter().next())
                .expect("ASG exists");
            let instance_id = group
                .instances
                .and_then(|i| i.members.into_iter().next())
                .and_then(|i| i.instance_id);
            if let Some(instance_id) = instance_id {
                self.instance_ids.push(instance_id.clone());
                let response = self
                    .ec2
                    .describe_instances(
                        DescribeInstancesRequest::builder()
                            .instance_ids(vec![instance_id.clone()])
                            .build(),
                    )
                    .await
                    .expect("describe launched instance");
                let instance = instances(response)
                    .into_iter()
                    .next()
                    .expect("launched instance is visible");
                // The instance profile is associated shortly after launch.
                if instance.iam_instance_profile.is_some() {
                    return instance;
                }
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        panic!("ASG {asg_name} did not launch an instance within 10 minutes");
    }
}

fn instances(response: DescribeInstancesResponse) -> Vec<Instance> {
    response
        .reservation_set
        .map(|set| {
            set.items
                .into_iter()
                .flat_map(|r| r.instances_set.map(|s| s.items).unwrap_or_default())
                .collect()
        })
        .unwrap_or_default()
}

fn launch_template_data(
    image_id: Option<&str>,
    security_group_ids: Option<Vec<String>>,
    instance_type: &str,
    instance_profile_arn: &str,
    root_volume_gib: i32,
) -> RequestLaunchTemplateData {
    RequestLaunchTemplateData::builder()
        .maybe_image_id(image_id.map(str::to_string))
        .maybe_security_group_ids(security_group_ids)
        .instance_type(instance_type.to_string())
        .iam_instance_profile(
            LaunchTemplateIamInstanceProfileSpecification::builder()
                .arn(instance_profile_arn.to_string())
                .build(),
        )
        .block_device_mappings(vec![LaunchTemplateBlockDeviceMapping::builder()
            .device_name(ROOT_DEVICE.to_string())
            .ebs(
                LaunchTemplateEbsBlockDevice::builder()
                    .volume_size(root_volume_gib)
                    .volume_type("gp3".to_string())
                    .delete_on_termination(true)
                    .build(),
            )
            .build()])
        .build()
}

#[test_context(LaunchTemplateContext)]
#[tokio::test]
async fn source_version_overrides_reach_launched_instances(ctx: &mut LaunchTemplateContext) {
    let (vpc_id, subnet_ids) = ctx.default_vpc().await;
    let profile_a = ctx.create_instance_profile("a").await;
    let profile_b = ctx.create_instance_profile("b").await;

    let security_group_id = ctx
        .ec2
        .create_security_group(
            CreateSecurityGroupRequest::builder()
                .group_name(format!("{}-sg", ctx.prefix))
                .description("launch template version test".to_string())
                .vpc_id(vpc_id)
                .build(),
        )
        .await
        .expect("create security group")
        .group_id
        .expect("security group ID");
    ctx.security_group_id = Some(security_group_id.clone());

    // Version 1: profile A, 8 GiB root volume, t3.micro.
    let launch_template_id = ctx
        .ec2
        .create_launch_template(
            CreateLaunchTemplateRequest::builder()
                .launch_template_name(ctx.prefix.clone())
                .launch_template_data(launch_template_data(
                    Some(AMI),
                    Some(vec![security_group_id.clone()]),
                    "t3.micro",
                    &profile_a,
                    8,
                ))
                .build(),
        )
        .await
        .expect("create launch template")
        .launch_template
        .and_then(|t| t.launch_template_id)
        .expect("launch template ID");
    ctx.launch_template_id = Some(launch_template_id.clone());

    // Version 2 from version 1: profile B, 12 GiB root volume, t3.small. The security
    // group is omitted and must be inherited.
    let version = ctx
        .ec2
        .create_launch_template_version(
            CreateLaunchTemplateVersionRequest::builder()
                .launch_template_id(launch_template_id.clone())
                .source_version("1".to_string())
                .launch_template_data(launch_template_data(None, None, "t3.small", &profile_b, 12))
                .build(),
        )
        .await
        .expect("create launch template version")
        .launch_template_version
        .expect("launch template version");
    assert_eq!(version.version_number, Some(2));
    info!(?version, "created launch template version 2");
    assert_eq!(
        version
            .launch_template_data
            .and_then(|d| d.iam_instance_profile)
            .and_then(|p| p.arn)
            .as_deref(),
        Some(profile_b.as_str()),
        "EC2 stored a different instance profile on version 2"
    );

    let asg_name = ctx.prefix.clone();
    ctx.asg
        .create_auto_scaling_group(
            CreateAutoScalingGroupRequest::builder()
                .auto_scaling_group_name(asg_name.clone())
                .launch_template(
                    LaunchTemplateSpecification::builder()
                        .launch_template_id(launch_template_id)
                        .version("$Latest".to_string())
                        .build(),
                )
                .min_size(1)
                .max_size(1)
                .desired_capacity(1)
                .vpc_zone_identifier(subnet_ids.join(","))
                .build(),
        )
        .await
        .expect("create ASG");
    ctx.asg_name = Some(asg_name.clone());

    let instance = ctx.wait_for_launched_instance(&asg_name).await;
    let instance_id = instance.instance_id.clone().expect("instance ID");
    info!(?instance, "instance launched from version 2");

    assert_eq!(instance.instance_type.as_deref(), Some("t3.small"));
    let attached_profile = instance
        .iam_instance_profile
        .and_then(|p| p.arn)
        .expect("instance has an instance profile");
    assert_eq!(
        attached_profile, profile_b,
        "the instance launched with the source version's instance profile"
    );

    let root_volume = ctx
        .ec2
        .describe_volumes(
            DescribeVolumesRequest::builder()
                .filters(vec![
                    Filter::builder()
                        .name("attachment.instance-id".to_string())
                        .values(vec![instance_id.clone()])
                        .build(),
                    Filter::builder()
                        .name("attachment.device".to_string())
                        .values(vec![ROOT_DEVICE.to_string()])
                        .build(),
                ])
                .build(),
        )
        .await
        .expect("describe root volume")
        .volume_set
        .and_then(|set| set.items.into_iter().next())
        .expect("instance has a root volume");
    info!(?root_volume, "root volume of the launched instance");
    assert_eq!(
        root_volume.size,
        Some(12),
        "the instance launched with the source version's root volume size"
    );
    assert_eq!(root_volume.volume_type.as_deref(), Some("gp3"));
}
