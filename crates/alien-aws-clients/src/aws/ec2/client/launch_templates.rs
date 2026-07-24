use super::*;

impl Ec2Client {
    pub(super) async fn create_launch_template_impl(
        &self,
        request: CreateLaunchTemplateRequest,
    ) -> Result<CreateLaunchTemplateResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "CreateLaunchTemplate".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert(
            "LaunchTemplateName".to_string(),
            request.launch_template_name.clone(),
        );

        if let Some(version_description) = &request.version_description {
            form_data.insert(
                "VersionDescription".to_string(),
                version_description.clone(),
            );
        }

        // Add launch template data
        let data = &request.launch_template_data;

        if let Some(image_id) = &data.image_id {
            form_data.insert("LaunchTemplateData.ImageId".to_string(), image_id.clone());
        }

        if let Some(instance_type) = &data.instance_type {
            form_data.insert(
                "LaunchTemplateData.InstanceType".to_string(),
                instance_type.clone(),
            );
        }

        if let Some(key_name) = &data.key_name {
            form_data.insert("LaunchTemplateData.KeyName".to_string(), key_name.clone());
        }

        if let Some(user_data) = &data.user_data {
            form_data.insert("LaunchTemplateData.UserData".to_string(), user_data.clone());
        }

        if let Some(security_group_ids) = &data.security_group_ids {
            for (i, sg_id) in security_group_ids.iter().enumerate() {
                form_data.insert(
                    format!("LaunchTemplateData.SecurityGroupId.{}", i + 1),
                    sg_id.clone(),
                );
            }
        }

        if let Some(iam_instance_profile) = &data.iam_instance_profile {
            if let Some(arn) = &iam_instance_profile.arn {
                form_data.insert(
                    "LaunchTemplateData.IamInstanceProfile.Arn".to_string(),
                    arn.clone(),
                );
            }
            if let Some(name) = &iam_instance_profile.name {
                form_data.insert(
                    "LaunchTemplateData.IamInstanceProfile.Name".to_string(),
                    name.clone(),
                );
            }
        }

        if let Some(block_device_mappings) = &data.block_device_mappings {
            for (i, bdm) in block_device_mappings.iter().enumerate() {
                let idx = i + 1;
                if let Some(device_name) = &bdm.device_name {
                    form_data.insert(
                        format!("LaunchTemplateData.BlockDeviceMapping.{}.DeviceName", idx),
                        device_name.clone(),
                    );
                }
                if let Some(ebs) = &bdm.ebs {
                    if let Some(volume_size) = ebs.volume_size {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.VolumeSize",
                                idx
                            ),
                            volume_size.to_string(),
                        );
                    }
                    if let Some(volume_type) = &ebs.volume_type {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.VolumeType",
                                idx
                            ),
                            volume_type.clone(),
                        );
                    }
                    if let Some(delete_on_termination) = ebs.delete_on_termination {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.DeleteOnTermination",
                                idx
                            ),
                            delete_on_termination.to_string(),
                        );
                    }
                    if let Some(encrypted) = ebs.encrypted {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.Encrypted",
                                idx
                            ),
                            encrypted.to_string(),
                        );
                    }
                    if let Some(iops) = ebs.iops {
                        form_data.insert(
                            format!("LaunchTemplateData.BlockDeviceMapping.{}.Ebs.Iops", idx),
                            iops.to_string(),
                        );
                    }
                    if let Some(throughput) = ebs.throughput {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.BlockDeviceMapping.{}.Ebs.Throughput",
                                idx
                            ),
                            throughput.to_string(),
                        );
                    }
                }
            }
        }

        if let Some(network_interfaces) = &data.network_interfaces {
            for (i, ni) in network_interfaces.iter().enumerate() {
                let idx = i + 1;
                if let Some(device_index) = ni.device_index {
                    form_data.insert(
                        format!("LaunchTemplateData.NetworkInterface.{}.DeviceIndex", idx),
                        device_index.to_string(),
                    );
                }
                if let Some(associate_public_ip) = ni.associate_public_ip_address {
                    form_data.insert(
                        format!(
                            "LaunchTemplateData.NetworkInterface.{}.AssociatePublicIpAddress",
                            idx
                        ),
                        associate_public_ip.to_string(),
                    );
                }
                if let Some(subnet_id) = &ni.subnet_id {
                    form_data.insert(
                        format!("LaunchTemplateData.NetworkInterface.{}.SubnetId", idx),
                        subnet_id.clone(),
                    );
                }
                if let Some(groups) = &ni.groups {
                    for (j, group) in groups.iter().enumerate() {
                        form_data.insert(
                            format!(
                                "LaunchTemplateData.NetworkInterface.{}.SecurityGroupId.{}",
                                idx,
                                j + 1
                            ),
                            group.clone(),
                        );
                    }
                }
            }
        }

        if let Some(metadata_options) = &data.metadata_options {
            if let Some(http_tokens) = &metadata_options.http_tokens {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.HttpTokens".to_string(),
                    http_tokens.clone(),
                );
            }
            if let Some(http_endpoint) = &metadata_options.http_endpoint {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.HttpEndpoint".to_string(),
                    http_endpoint.clone(),
                );
            }
            if let Some(http_put_response_hop_limit) = metadata_options.http_put_response_hop_limit
            {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.HttpPutResponseHopLimit".to_string(),
                    http_put_response_hop_limit.to_string(),
                );
            }
            if let Some(instance_metadata_tags) = &metadata_options.instance_metadata_tags {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.InstanceMetadataTags".to_string(),
                    instance_metadata_tags.clone(),
                );
            }
        }

        Self::add_cpu_options(&mut form_data, data.cpu_options.as_ref());

        if let Some(tag_specs) = &data.tag_specifications {
            Self::add_tag_specifications_with_prefix(
                &mut form_data,
                "LaunchTemplateData.TagSpecification",
                tag_specs,
            );
        }

        if let Some(tag_specs) = &request.tag_specifications {
            Self::add_tag_specifications(&mut form_data, tag_specs);
        }

        self.send_form(
            form_data,
            "CreateLaunchTemplate",
            &request.launch_template_name,
        )
        .await
    }

    pub(super) async fn create_launch_template_version_impl(
        &self,
        request: CreateLaunchTemplateVersionRequest,
    ) -> Result<CreateLaunchTemplateVersionResponse> {
        let mut form_data = HashMap::new();
        form_data.insert(
            "Action".to_string(),
            "CreateLaunchTemplateVersion".to_string(),
        );
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        let resource_name;
        if let Some(ref id) = request.launch_template_id {
            form_data.insert("LaunchTemplateId".to_string(), id.clone());
            resource_name = id.clone();
        } else if let Some(ref name) = request.launch_template_name {
            form_data.insert("LaunchTemplateName".to_string(), name.clone());
            resource_name = name.clone();
        } else {
            return Err(alien_error::AlienError::new(ErrorData::InvalidInput {
                message: "Either launch_template_id or launch_template_name must be provided"
                    .to_string(),
                field_name: Some("launch_template_id".to_string()),
            }));
        }

        if let Some(ref source_version) = request.source_version {
            form_data.insert("SourceVersion".to_string(), source_version.clone());
        }
        if let Some(ref description) = request.version_description {
            form_data.insert("VersionDescription".to_string(), description.clone());
        }

        let data = &request.launch_template_data;
        if let Some(ref user_data) = data.user_data {
            form_data.insert("LaunchTemplateData.UserData".to_string(), user_data.clone());
        }
        if let Some(ref image_id) = data.image_id {
            form_data.insert("LaunchTemplateData.ImageId".to_string(), image_id.clone());
        }
        if let Some(ref instance_type) = data.instance_type {
            form_data.insert(
                "LaunchTemplateData.InstanceType".to_string(),
                instance_type.clone(),
            );
        }
        if let Some(metadata_options) = &data.metadata_options {
            if let Some(http_tokens) = &metadata_options.http_tokens {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.HttpTokens".to_string(),
                    http_tokens.clone(),
                );
            }
            if let Some(http_endpoint) = &metadata_options.http_endpoint {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.HttpEndpoint".to_string(),
                    http_endpoint.clone(),
                );
            }
            if let Some(http_put_response_hop_limit) = metadata_options.http_put_response_hop_limit
            {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.HttpPutResponseHopLimit".to_string(),
                    http_put_response_hop_limit.to_string(),
                );
            }
            if let Some(instance_metadata_tags) = &metadata_options.instance_metadata_tags {
                form_data.insert(
                    "LaunchTemplateData.MetadataOptions.InstanceMetadataTags".to_string(),
                    instance_metadata_tags.clone(),
                );
            }
        }
        Self::add_cpu_options(&mut form_data, data.cpu_options.as_ref());
        if let Some(tag_specs) = &data.tag_specifications {
            Self::add_tag_specifications_with_prefix(
                &mut form_data,
                "LaunchTemplateData.TagSpecification",
                tag_specs,
            );
        }

        self.send_form(form_data, "CreateLaunchTemplateVersion", &resource_name)
            .await
    }

    pub(super) async fn delete_launch_template_impl(
        &self,
        request: DeleteLaunchTemplateRequest,
    ) -> Result<DeleteLaunchTemplateResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteLaunchTemplate".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        let resource: String;
        if let Some(launch_template_id) = &request.launch_template_id {
            form_data.insert("LaunchTemplateId".to_string(), launch_template_id.clone());
            resource = launch_template_id.clone();
        } else if let Some(launch_template_name) = &request.launch_template_name {
            form_data.insert(
                "LaunchTemplateName".to_string(),
                launch_template_name.clone(),
            );
            resource = launch_template_name.clone();
        } else {
            resource = "unknown".to_string();
        }

        self.send_form(form_data, "DeleteLaunchTemplate", &resource)
            .await
    }

    pub(super) async fn describe_launch_templates_impl(
        &self,
        request: DescribeLaunchTemplatesRequest,
    ) -> Result<DescribeLaunchTemplatesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeLaunchTemplates".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(launch_template_ids) = &request.launch_template_ids {
            for (i, lt_id) in launch_template_ids.iter().enumerate() {
                form_data.insert(format!("LaunchTemplateId.{}", i + 1), lt_id.clone());
            }
        }

        if let Some(launch_template_names) = &request.launch_template_names {
            for (i, lt_name) in launch_template_names.iter().enumerate() {
                form_data.insert(format!("LaunchTemplateName.{}", i + 1), lt_name.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeLaunchTemplates", "LaunchTemplate")
            .await
    }

    pub(super) async fn get_console_output_impl(
        &self,
        instance_id: String,
    ) -> Result<GetConsoleOutputResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "GetConsoleOutput".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("InstanceId".to_string(), instance_id.clone());

        self.send_form(form_data, "GetConsoleOutput", &instance_id)
            .await
    }
}
