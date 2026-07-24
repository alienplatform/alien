use super::*;

impl Ec2Client {
    pub(super) async fn describe_images_impl(
        &self,
        request: DescribeImagesRequest,
    ) -> Result<DescribeImagesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeImages".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(image_ids) = &request.image_ids {
            for (i, image_id) in image_ids.iter().enumerate() {
                form_data.insert(format!("ImageId.{}", i + 1), image_id.clone());
            }
        }

        if let Some(owners) = &request.owners {
            for (i, owner) in owners.iter().enumerate() {
                form_data.insert(format!("Owner.{}", i + 1), owner.clone());
            }
        }

        if let Some(executable_users) = &request.executable_users {
            for (i, user) in executable_users.iter().enumerate() {
                form_data.insert(format!("ExecutableBy.{}", i + 1), user.clone());
            }
        }

        if let Some(filters) = &request.filters {
            Self::add_filters(&mut form_data, filters);
        }

        if let Some(include_deprecated) = request.include_deprecated {
            form_data.insert(
                "IncludeDeprecated".to_string(),
                include_deprecated.to_string(),
            );
        }

        if let Some(max_results) = request.max_results {
            form_data.insert("MaxResults".to_string(), max_results.to_string());
        }

        if let Some(next_token) = &request.next_token {
            form_data.insert("NextToken".to_string(), next_token.clone());
        }

        self.send_form(form_data, "DescribeImages", "AMI").await
    }

    // ---------------------------------------------------------------------------
    // Instance Operations
    // ---------------------------------------------------------------------------

    pub(super) async fn terminate_instances_impl(
        &self,
        instance_ids: Vec<String>,
    ) -> Result<TerminateInstancesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "TerminateInstances".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        for (i, instance_id) in instance_ids.iter().enumerate() {
            form_data.insert(format!("InstanceId.{}", i + 1), instance_id.clone());
        }

        let resource = instance_ids.join(",");
        self.send_form(form_data, "TerminateInstances", &resource)
            .await
    }

    pub(super) async fn describe_instances_impl(
        &self,
        request: DescribeInstancesRequest,
    ) -> Result<DescribeInstancesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeInstances".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(instance_ids) = &request.instance_ids {
            for (i, instance_id) in instance_ids.iter().enumerate() {
                form_data.insert(format!("InstanceId.{}", i + 1), instance_id.clone());
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

        self.send_form(form_data, "DescribeInstances", "Instance")
            .await
    }

    // ---------------------------------------------------------------------------
    // Volume Operations
    // ---------------------------------------------------------------------------

    pub(super) async fn create_volume_impl(
        &self,
        request: CreateVolumeRequest,
    ) -> Result<CreateVolumeResponse> {
        let form_data = Self::create_volume_form_data(&request);

        self.send_form(form_data, "CreateVolume", &request.availability_zone)
            .await
    }

    pub(super) async fn modify_volume_impl(
        &self,
        request: ModifyVolumeRequest,
    ) -> Result<ModifyVolumeResponse> {
        let form_data = Self::modify_volume_form_data(&request);
        self.send_form(form_data, "ModifyVolume", &request.volume_id)
            .await
    }

    pub(super) async fn describe_volumes_modifications_impl(
        &self,
        request: DescribeVolumesModificationsRequest,
    ) -> Result<DescribeVolumesModificationsResponse> {
        let form_data = Self::describe_volumes_modifications_form_data(&request);
        self.send_form(
            form_data,
            "DescribeVolumesModifications",
            "VolumeModification",
        )
        .await
    }

    pub(super) async fn delete_volume_impl(&self, volume_id: &str) -> Result<()> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DeleteVolume".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VolumeId".to_string(), volume_id.to_string());

        self.send_form_no_body(form_data, "DeleteVolume", volume_id)
            .await
    }

    pub(super) async fn describe_volumes_impl(
        &self,
        request: DescribeVolumesRequest,
    ) -> Result<DescribeVolumesResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DescribeVolumes".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());

        if let Some(volume_ids) = &request.volume_ids {
            for (i, volume_id) in volume_ids.iter().enumerate() {
                form_data.insert(format!("VolumeId.{}", i + 1), volume_id.clone());
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

        self.send_form(form_data, "DescribeVolumes", "Volume").await
    }

    pub(super) async fn attach_volume_impl(
        &self,
        request: AttachVolumeRequest,
    ) -> Result<AttachVolumeResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "AttachVolume".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VolumeId".to_string(), request.volume_id.clone());
        form_data.insert("InstanceId".to_string(), request.instance_id.clone());
        form_data.insert("Device".to_string(), request.device.clone());

        let resource = format!("{}:{}", request.volume_id, request.instance_id);
        self.send_form(form_data, "AttachVolume", &resource).await
    }

    pub(super) async fn detach_volume_impl(
        &self,
        request: DetachVolumeRequest,
    ) -> Result<DetachVolumeResponse> {
        let mut form_data = HashMap::new();
        form_data.insert("Action".to_string(), "DetachVolume".to_string());
        form_data.insert("Version".to_string(), "2016-11-15".to_string());
        form_data.insert("VolumeId".to_string(), request.volume_id.clone());

        if let Some(instance_id) = &request.instance_id {
            form_data.insert("InstanceId".to_string(), instance_id.clone());
        }

        if let Some(device) = &request.device {
            form_data.insert("Device".to_string(), device.clone());
        }

        if let Some(force) = request.force {
            form_data.insert("Force".to_string(), force.to_string());
        }

        self.send_form(form_data, "DetachVolume", &request.volume_id)
            .await
    }

    // ---------------------------------------------------------------------------
    // Launch Template Operations
    // ---------------------------------------------------------------------------
}
