use std::collections::{BTreeMap, HashMap};

use alien_aws_clients::{
    ec2::{
        CreateLaunchTemplateRequest, CreateLaunchTemplateVersionRequest, DescribeInstancesRequest,
        LaunchTemplateBlockDeviceMapping, LaunchTemplateCpuOptions, LaunchTemplateEbsBlockDevice,
        LaunchTemplateIamInstanceProfileSpecification, LaunchTemplateInstanceMetadataOptions,
        LaunchTemplateNetworkInterface, RequestLaunchTemplateData, Tag, TagSpecification,
    },
    AwsClientConfig, AwsCredentialProvider, AwsCredentials, Ec2Api, Ec2Client, ServiceOverrides,
};
use httpmock::prelude::*;

fn client(server: &MockServer) -> Ec2Client {
    Ec2Client::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".into(),
                secret_access_key: "test-secret-key".into(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("ec2".into(), server.base_url())]),
            }),
        }),
    )
}

fn complete_data() -> RequestLaunchTemplateData {
    RequestLaunchTemplateData {
        image_id: Some("ami-test".into()),
        instance_type: Some("m8i.2xlarge".into()),
        key_name: Some("ssh-key".into()),
        user_data: Some("dGVzdA==".into()),
        security_group_ids: Some(vec!["sg-one".into(), "sg-two".into()]),
        iam_instance_profile: Some(LaunchTemplateIamInstanceProfileSpecification {
            arn: Some("arn:aws:iam::123456789012:instance-profile/test-profile".into()),
            name: None,
        }),
        block_device_mappings: Some(vec![
            LaunchTemplateBlockDeviceMapping {
                device_name: Some("/dev/sda1".into()),
                ebs: Some(LaunchTemplateEbsBlockDevice {
                    volume_size: Some(32),
                    volume_type: Some("gp3".into()),
                    delete_on_termination: Some(false),
                    encrypted: Some(true),
                    iops: Some(3000),
                    throughput: Some(125),
                }),
            },
            LaunchTemplateBlockDeviceMapping {
                device_name: Some("/dev/sdf".into()),
                ebs: None,
            },
        ]),
        network_interfaces: Some(vec![
            LaunchTemplateNetworkInterface {
                device_index: Some(0),
                associate_public_ip_address: Some(false),
                subnet_id: Some("subnet-one".into()),
                groups: Some(vec!["sg-three".into(), "sg-four".into()]),
            },
            LaunchTemplateNetworkInterface {
                device_index: Some(1),
                associate_public_ip_address: None,
                subnet_id: Some("subnet-two".into()),
                groups: None,
            },
        ]),
        metadata_options: Some(LaunchTemplateInstanceMetadataOptions {
            http_tokens: Some("required".into()),
            http_endpoint: Some("enabled".into()),
            http_put_response_hop_limit: Some(1),
            instance_metadata_tags: Some("disabled".into()),
        }),
        cpu_options: Some(LaunchTemplateCpuOptions {
            nested_virtualization: Some("enabled".into()),
        }),
        tag_specifications: Some(vec![
            TagSpecification {
                resource_type: "instance".into(),
                tags: vec![
                    Tag {
                        key: "Name".into(),
                        value: "test instance + one".into(),
                    },
                    Tag {
                        key: "purpose".into(),
                        value: "test".into(),
                    },
                ],
            },
            TagSpecification {
                resource_type: "volume".into(),
                tags: vec![Tag {
                    key: "Name".into(),
                    value: "test-volume".into(),
                }],
            },
        ]),
    }
}

fn complete_fields() -> BTreeMap<String, String> {
    [
        ("ImageId", "ami-test"),
        ("InstanceType", "m8i.2xlarge"),
        ("KeyName", "ssh-key"),
        ("UserData", "dGVzdA=="),
        ("SecurityGroupId.1", "sg-one"),
        ("SecurityGroupId.2", "sg-two"),
        (
            "IamInstanceProfile.Arn",
            "arn:aws:iam::123456789012:instance-profile/test-profile",
        ),
        ("BlockDeviceMapping.1.DeviceName", "/dev/sda1"),
        ("BlockDeviceMapping.1.Ebs.VolumeSize", "32"),
        ("BlockDeviceMapping.1.Ebs.VolumeType", "gp3"),
        ("BlockDeviceMapping.1.Ebs.DeleteOnTermination", "false"),
        ("BlockDeviceMapping.1.Ebs.Encrypted", "true"),
        ("BlockDeviceMapping.1.Ebs.Iops", "3000"),
        ("BlockDeviceMapping.1.Ebs.Throughput", "125"),
        ("BlockDeviceMapping.2.DeviceName", "/dev/sdf"),
        ("NetworkInterface.1.DeviceIndex", "0"),
        ("NetworkInterface.1.AssociatePublicIpAddress", "false"),
        ("NetworkInterface.1.SubnetId", "subnet-one"),
        ("NetworkInterface.1.SecurityGroupId.1", "sg-three"),
        ("NetworkInterface.1.SecurityGroupId.2", "sg-four"),
        ("NetworkInterface.2.DeviceIndex", "1"),
        ("NetworkInterface.2.SubnetId", "subnet-two"),
        ("MetadataOptions.HttpTokens", "required"),
        ("MetadataOptions.HttpEndpoint", "enabled"),
        ("MetadataOptions.HttpPutResponseHopLimit", "1"),
        ("MetadataOptions.InstanceMetadataTags", "disabled"),
        ("CpuOptions.NestedVirtualization", "enabled"),
        ("TagSpecification.1.ResourceType", "instance"),
        ("TagSpecification.1.Tag.1.Key", "Name"),
        ("TagSpecification.1.Tag.1.Value", "test instance + one"),
        ("TagSpecification.1.Tag.2.Key", "purpose"),
        ("TagSpecification.1.Tag.2.Value", "test"),
        ("TagSpecification.2.ResourceType", "volume"),
        ("TagSpecification.2.Tag.1.Key", "Name"),
        ("TagSpecification.2.Tag.1.Value", "test-volume"),
    ]
    .into_iter()
    .map(|(key, value)| (format!("LaunchTemplateData.{key}"), value.into()))
    .collect()
}

fn form(request: &HttpMockRequest) -> BTreeMap<String, String> {
    form_urlencoded::parse(request.body.as_deref().unwrap_or_default())
        .into_owned()
        .collect()
}

fn complete_request(request: &HttpMockRequest) -> bool {
    let actual = form(request);
    let mut expected = complete_fields();
    expected.insert("Version".into(), "2016-11-15".into());
    expected.insert("VersionDescription".into(), "test version".into());
    match actual.get("Action").map(String::as_str) {
        Some("CreateLaunchTemplate") => {
            expected.insert("Action".into(), "CreateLaunchTemplate".into());
            expected.insert("LaunchTemplateName".into(), "test-template".into());
            expected.insert(
                "TagSpecification.1.ResourceType".into(),
                "launch-template".into(),
            );
            expected.insert("TagSpecification.1.Tag.1.Key".into(), "Name".into());
            expected.insert(
                "TagSpecification.1.Tag.1.Value".into(),
                "template-tag".into(),
            );
        }
        Some("CreateLaunchTemplateVersion") => {
            expected.insert("Action".into(), "CreateLaunchTemplateVersion".into());
            expected.insert("LaunchTemplateId".into(), "lt-test".into());
            expected.insert("SourceVersion".into(), "$Latest".into());
        }
        _ => return false,
    }
    actual == expected
}

#[tokio::test]
async fn both_create_apis_send_every_modeled_launch_template_field() {
    for version in [false, true] {
        let server = MockServer::start_async().await;
        let request = server.mock_async(|when, then| {
            when.method(POST).path("/").header_exists("authorization").matches(complete_request);
            then.status(200).body(if version {
                "<CreateLaunchTemplateVersionResponse><launchTemplateVersion><launchTemplateId>lt-test</launchTemplateId><versionNumber>2</versionNumber><launchTemplateData><iamInstanceProfile><arn>arn:aws:iam::123456789012:instance-profile/test-profile</arn><name>test-profile</name></iamInstanceProfile></launchTemplateData></launchTemplateVersion></CreateLaunchTemplateVersionResponse>"
            } else {
                "<CreateLaunchTemplateResponse/>"
            });
        }).await;
        let client = client(&server);
        if version {
            let result = client
                .create_launch_template_version(CreateLaunchTemplateVersionRequest {
                    launch_template_id: Some("lt-test".into()),
                    source_version: Some("$Latest".into()),
                    version_description: Some("test version".into()),
                    launch_template_data: complete_data(),
                    ..Default::default()
                })
                .await
                .unwrap();
            let version = result.launch_template_version.unwrap();
            assert_eq!(version.version_number, Some(2));
            assert_eq!(version.launch_template_id.as_deref(), Some("lt-test"));
            let profile = version
                .launch_template_data
                .unwrap()
                .iam_instance_profile
                .unwrap();
            assert_eq!(
                profile.arn.as_deref(),
                Some("arn:aws:iam::123456789012:instance-profile/test-profile")
            );
            assert_eq!(profile.name.as_deref(), Some("test-profile"));
        } else {
            client
                .create_launch_template(CreateLaunchTemplateRequest {
                    launch_template_name: "test-template".into(),
                    version_description: Some("test version".into()),
                    launch_template_data: complete_data(),
                    tag_specifications: Some(vec![TagSpecification {
                        resource_type: "launch-template".into(),
                        tags: vec![Tag {
                            key: "Name".into(),
                            value: "template-tag".into(),
                        }],
                    }]),
                })
                .await
                .unwrap();
        }
        request.assert_hits_async(1).await;
    }
}

fn name_only_request(request: &HttpMockRequest) -> bool {
    form(request)
        == BTreeMap::from([
            ("Action".into(), "CreateLaunchTemplateVersion".into()),
            ("Version".into(), "2016-11-15".into()),
            ("LaunchTemplateName".into(), "test-template".into()),
            (
                "LaunchTemplateData.IamInstanceProfile.Name".into(),
                "test-profile".into(),
            ),
        ])
}

#[tokio::test]
async fn version_can_override_profile_by_name_without_an_arn_or_source_version() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .header_exists("authorization")
                .matches(name_only_request);
            then.status(200)
                .body("<CreateLaunchTemplateVersionResponse/>");
        })
        .await;
    client(&server)
        .create_launch_template_version(CreateLaunchTemplateVersionRequest {
            launch_template_name: Some("test-template".into()),
            launch_template_data: RequestLaunchTemplateData {
                iam_instance_profile: Some(LaunchTemplateIamInstanceProfileSpecification {
                    arn: None,
                    name: Some("test-profile".into()),
                }),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .unwrap();
    request.assert_hits_async(1).await;
}

fn inherited_request(request: &HttpMockRequest) -> bool {
    form(request)
        == BTreeMap::from([
            ("Action".into(), "CreateLaunchTemplateVersion".into()),
            ("Version".into(), "2016-11-15".into()),
            ("LaunchTemplateId".into(), "lt-test".into()),
            ("SourceVersion".into(), "$Latest".into()),
            ("LaunchTemplateData.ImageId".into(), "ami-new".into()),
        ])
}

#[tokio::test]
async fn sparse_version_does_not_send_overrides_for_inherited_fields() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .header_exists("authorization")
                .matches(inherited_request);
            then.status(200)
                .body("<CreateLaunchTemplateVersionResponse/>");
        })
        .await;
    client(&server)
        .create_launch_template_version(CreateLaunchTemplateVersionRequest {
            launch_template_id: Some("lt-test".into()),
            source_version: Some("$Latest".into()),
            launch_template_data: RequestLaunchTemplateData {
                image_id: Some("ami-new".into()),
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .unwrap();
    request.assert_hits_async(1).await;
}

#[tokio::test]
async fn describe_instances_decodes_attached_profile_and_preserves_profileless_instances() {
    let server = MockServer::start_async().await;
    let request = server.mock_async(|when, then| {
        when.method(POST).path("/").header_exists("authorization")
            .x_www_form_urlencoded_tuple("Action", "DescribeInstances")
            .x_www_form_urlencoded_tuple("InstanceId.1", "i-profile")
            .x_www_form_urlencoded_tuple("InstanceId.2", "i-no-profile");
        then.status(200).body(r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
            <requestId>test-request</requestId>
            <reservationSet><item><reservationId>r-test</reservationId><ownerId>123456789012</ownerId>
                <instancesSet>
                    <item><instanceId>i-profile</instanceId><imageId>ami-test</imageId>
                        <instanceState><code>16</code><name>running</name></instanceState>
                        <iamInstanceProfile><arn>arn:aws:iam::123456789012:instance-profile/test-profile</arn><id>AIPATEST</id></iamInstanceProfile>
                    </item>
                    <item><instanceId>i-no-profile</instanceId><imageId>ami-test</imageId></item>
                </instancesSet>
            </item></reservationSet>
            <nextToken>next-page</nextToken>
        </DescribeInstancesResponse>"#);
    }).await;
    let result = client(&server)
        .describe_instances(DescribeInstancesRequest {
            instance_ids: Some(vec!["i-profile".into(), "i-no-profile".into()]),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(result.next_token.as_deref(), Some("next-page"));
    let reservation = &result.reservation_set.unwrap().items[0];
    assert_eq!(reservation.reservation_id.as_deref(), Some("r-test"));
    let instances = &reservation.instances_set.as_ref().unwrap().items;
    assert_eq!(instances.len(), 2);
    assert_eq!(instances[0].instance_id.as_deref(), Some("i-profile"));
    assert_eq!(
        instances[0]
            .instance_state
            .as_ref()
            .unwrap()
            .name
            .as_deref(),
        Some("running")
    );
    let profile = instances[0].iam_instance_profile.as_ref().unwrap();
    assert_eq!(
        profile.arn.as_deref(),
        Some("arn:aws:iam::123456789012:instance-profile/test-profile")
    );
    assert_eq!(profile.id.as_deref(), Some("AIPATEST"));
    assert_eq!(instances[1].instance_id.as_deref(), Some("i-no-profile"));
    assert!(instances[1].iam_instance_profile.is_none());
    request.assert_hits_async(1).await;
}
