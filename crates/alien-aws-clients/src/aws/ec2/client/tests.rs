use super::*;

#[test]
fn create_volume_maps_idempotency_token_to_ec2_query_parameter() {
    let request = CreateVolumeRequest::builder()
        .availability_zone("us-west-2a".to_string())
        .client_token("deployment-disk-ordinal-3".to_string())
        .size(64)
        .volume_type("gp3".to_string())
        .encrypted(true)
        .build();

    let form = Ec2Client::create_volume_form_data(&request);

    assert_eq!(form.get("Action").map(String::as_str), Some("CreateVolume"));
    assert_eq!(
        form.get("ClientToken").map(String::as_str),
        Some("deployment-disk-ordinal-3")
    );
    assert_eq!(
        form.get("AvailabilityZone").map(String::as_str),
        Some("us-west-2a")
    );
    assert_eq!(form.get("Size").map(String::as_str), Some("64"));
    assert_eq!(form.get("VolumeType").map(String::as_str), Some("gp3"));
    assert_eq!(form.get("Encrypted").map(String::as_str), Some("true"));
}

#[test]
fn volume_growth_operations_map_to_ec2_query_parameters() {
    let modify = ModifyVolumeRequest::builder()
        .volume_id("vol-0123456789abcdef0".to_string())
        .size(128)
        .build();
    let modify_form = Ec2Client::modify_volume_form_data(&modify);
    assert_eq!(
        modify_form.get("Action").map(String::as_str),
        Some("ModifyVolume")
    );
    assert_eq!(
        modify_form.get("VolumeId").map(String::as_str),
        Some("vol-0123456789abcdef0")
    );
    assert_eq!(modify_form.get("Size").map(String::as_str), Some("128"));

    let describe = DescribeVolumesModificationsRequest::builder()
        .volume_ids(vec![
            "vol-0123456789abcdef0".to_string(),
            "vol-0123456789abcdef1".to_string(),
        ])
        .max_results(2)
        .next_token("next-page".to_string())
        .build();
    let describe_form = Ec2Client::describe_volumes_modifications_form_data(&describe);
    assert_eq!(
        describe_form.get("Action").map(String::as_str),
        Some("DescribeVolumesModifications")
    );
    assert_eq!(
        describe_form.get("VolumeId.1").map(String::as_str),
        Some("vol-0123456789abcdef0")
    );
    assert_eq!(
        describe_form.get("VolumeId.2").map(String::as_str),
        Some("vol-0123456789abcdef1")
    );
    assert_eq!(
        describe_form.get("MaxResults").map(String::as_str),
        Some("2")
    );
    assert_eq!(
        describe_form.get("NextToken").map(String::as_str),
        Some("next-page")
    );
}

#[test]
fn volume_modification_responses_deserialize_from_ec2_xml() {
    let modify: ModifyVolumeResponse = quick_xml::de::from_str(
        r#"<ModifyVolumeResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
                <volumeModification>
                    <volumeId>vol-0123456789abcdef0</volumeId>
                    <modificationState>modifying</modificationState>
                    <progress>0</progress>
                    <originalSize>64</originalSize>
                    <targetSize>128</targetSize>
                </volumeModification>
            </ModifyVolumeResponse>"#,
    )
    .expect("ModifyVolume response should deserialize");
    let modification = modify
        .volume_modification
        .expect("ModifyVolume should include its modification");
    assert_eq!(
        modification.volume_id.as_deref(),
        Some("vol-0123456789abcdef0")
    );
    assert_eq!(
        modification.modification_state.as_deref(),
        Some("modifying")
    );
    assert_eq!(modification.progress, Some(0));
    assert_eq!(modification.original_size, Some(64));
    assert_eq!(modification.target_size, Some(128));

    let described: DescribeVolumesModificationsResponse = quick_xml::de::from_str(
        r#"<DescribeVolumesModificationsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
                <volumeModificationSet>
                    <item>
                        <volumeId>vol-0123456789abcdef0</volumeId>
                        <modificationState>completed</modificationState>
                        <progress>100</progress>
                        <originalSize>64</originalSize>
                        <targetSize>128</targetSize>
                        <endTime>2026-07-20T10:00:00.000Z</endTime>
                    </item>
                </volumeModificationSet>
                <nextToken>next-page</nextToken>
            </DescribeVolumesModificationsResponse>"#,
    )
    .expect("DescribeVolumesModifications response should deserialize");
    let modifications = described
        .volume_modification_set
        .expect("response should include modifications");
    assert_eq!(modifications.items.len(), 1);
    assert_eq!(
        modifications.items[0].modification_state.as_deref(),
        Some("completed")
    );
    assert_eq!(modifications.items[0].progress, Some(100));
    assert_eq!(described.next_token.as_deref(), Some("next-page"));
}

#[test]
fn describe_volumes_deserializes_aws_status_as_volume_state() {
    let response: DescribeVolumesResponse = quick_xml::de::from_str(
        r#"<DescribeVolumesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
                <volumeSet>
                    <item>
                        <volumeId>vol-0123456789abcdef0</volumeId>
                        <size>10</size>
                        <availabilityZone>us-west-2a</availabilityZone>
                        <status>available</status>
                        <volumeType>gp3</volumeType>
                    </item>
                </volumeSet>
            </DescribeVolumesResponse>"#,
    )
    .expect("AWS DescribeVolumes response should deserialize");

    let volumes = response
        .volume_set
        .expect("volumeSet should be present")
        .items;
    assert_eq!(volumes.len(), 1);
    assert_eq!(
        volumes[0].volume_id.as_deref(),
        Some("vol-0123456789abcdef0")
    );
    assert_eq!(volumes[0].state.as_deref(), Some("available"));
}

#[test]
fn launch_template_already_exists_is_a_typed_resource_conflict() {
    let response = r#"<?xml version="1.0" encoding="UTF-8"?>
            <Response><Errors><Error>
                <Code>InvalidLaunchTemplateName.AlreadyExistsException</Code>
                <Message>Launch template name already in use.</Message>
            </Error></Errors><RequestID>request-id</RequestID></Response>"#;

    let error = Ec2Client::map_ec2_error(
        StatusCode::BAD_REQUEST,
        response,
        "CreateLaunchTemplate",
        "deployment-compute-general-lt",
        Some("LaunchTemplateName=deployment-compute-general-lt"),
    );

    assert!(matches!(
        error,
        Some(ErrorData::RemoteResourceConflict {
            resource_type,
            resource_name,
            ..
        }) if resource_type == "EC2 Resource"
            && resource_name == "deployment-compute-general-lt"
    ));
}

#[test]
fn missing_volume_is_a_typed_remote_resource_not_found() {
    let response = r#"<?xml version="1.0" encoding="UTF-8"?>
            <Response><Errors><Error>
                <Code>InvalidVolume.NotFound</Code>
                <Message>The volume does not exist.</Message>
            </Error></Errors><RequestID>request-id</RequestID></Response>"#;

    let error = Ec2Client::map_ec2_error(
        StatusCode::BAD_REQUEST,
        response,
        "DescribeVolumesModifications",
        "vol-0123456789abcdef0",
        Some("VolumeId.1=vol-0123456789abcdef0"),
    );

    assert!(matches!(
        error,
        Some(ErrorData::RemoteResourceNotFound {
            resource_type,
            resource_name,
        }) if resource_type == "Volume" && resource_name == "vol-0123456789abcdef0"
    ));
}

/// Setting `nested_virtualization=Some("enabled")` emits the AWS
/// form-encoded key `LaunchTemplateData.CpuOptions.NestedVirtualization`
/// with value `enabled`. Locks in the exact wire-format string AWS
/// rejects/accepts on the LT create endpoint.
#[test]
fn add_cpu_options_emits_nested_virtualization_key() {
    let mut form_data = HashMap::new();
    let cpu_options = LaunchTemplateCpuOptions::builder()
        .nested_virtualization("enabled".to_string())
        .build();
    Ec2Client::add_cpu_options(&mut form_data, Some(&cpu_options));
    assert_eq!(
        form_data.get("LaunchTemplateData.CpuOptions.NestedVirtualization"),
        Some(&"enabled".to_string())
    );
    assert_eq!(form_data.len(), 1);
}

/// `None` cpu_options → nothing is appended. Guards against accidentally
/// sending an empty `CpuOptions` block on non-privileged daemon deploys.
#[test]
fn add_cpu_options_with_none_emits_nothing() {
    let mut form_data = HashMap::new();
    Ec2Client::add_cpu_options(&mut form_data, None);
    assert!(form_data.is_empty());
}

/// `Some(CpuOptions { nested_virtualization: None })` → nothing appended.
/// Future fields on CpuOptions could be set independently; the encoder
/// should not emit a key for an unset NestedVirtualization specifically.
#[test]
fn add_cpu_options_with_unset_nested_field_emits_nothing() {
    let mut form_data = HashMap::new();
    let cpu_options = LaunchTemplateCpuOptions::builder().build();
    Ec2Client::add_cpu_options(&mut form_data, Some(&cpu_options));
    assert!(form_data.is_empty());
}
