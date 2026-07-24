use super::*;

impl ComputeClient {
    pub(super) async fn get_instance_template_impl(
        &self,
        instance_template_name: String,
    ) -> Result<InstanceTemplate> {
        let path = format!(
            "projects/{}/global/instanceTemplates/{}",
            self.project_id, instance_template_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &instance_template_name,
            )
            .await
    }

    pub(super) async fn insert_instance_template_impl(
        &self,
        instance_template: InstanceTemplate,
    ) -> Result<Operation> {
        let path = format!("projects/{}/global/instanceTemplates", self.project_id);
        let resource_name = instance_template.name.clone().unwrap_or_default();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(instance_template),
                &resource_name,
            )
            .await
    }

    pub(super) async fn delete_instance_template_impl(
        &self,
        instance_template_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/global/instanceTemplates/{}",
            self.project_id, instance_template_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &instance_template_name,
            )
            .await
    }

    // --- Instance Group Manager Operations ---

    pub(super) async fn get_instance_group_manager_impl(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<InstanceGroupManager> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &instance_group_manager_name,
            )
            .await
    }

    pub(super) async fn insert_instance_group_manager_impl(
        &self,
        zone: String,
        instance_group_manager: InstanceGroupManager,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers",
            self.project_id, zone
        );
        let resource_name = instance_group_manager.name.clone().unwrap_or_default();
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(instance_group_manager),
                &resource_name,
            )
            .await
    }

    pub(super) async fn delete_instance_group_manager_impl(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &instance_group_manager_name,
            )
            .await
    }

    pub(super) async fn resize_instance_group_manager_impl(
        &self,
        zone: String,
        instance_group_manager_name: String,
        size: i32,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}/resize",
            self.project_id, zone, instance_group_manager_name
        );
        let query_params = vec![("size", size.to_string())];
        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query_params),
                Option::<()>::None,
                &instance_group_manager_name,
            )
            .await
    }

    pub(super) async fn delete_instance_group_manager_instances_impl(
        &self,
        zone: String,
        instance_group_manager_name: String,
        request: InstanceGroupManagersDeleteInstancesRequest,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}/deleteInstances",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(request),
                &instance_group_manager_name,
            )
            .await
    }

    pub(super) async fn list_managed_instances_impl(
        &self,
        zone: String,
        instance_group_manager_name: String,
    ) -> Result<InstanceGroupManagersListManagedInstancesResponse> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}/listManagedInstances",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Option::<()>::None,
                &instance_group_manager_name,
            )
            .await
    }

    pub(super) async fn patch_instance_group_manager_impl(
        &self,
        zone: String,
        instance_group_manager_name: String,
        patch: InstanceGroupManager,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instanceGroupManagers/{}",
            self.project_id, zone, instance_group_manager_name
        );
        self.base
            .execute_request(
                Method::PATCH,
                &path,
                None,
                Some(patch),
                &instance_group_manager_name,
            )
            .await
    }

    // --- Instance Operations ---

    pub(super) async fn get_instance_impl(
        &self,
        zone: String,
        instance_name: String,
    ) -> Result<Instance> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}",
            self.project_id, zone, instance_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &instance_name)
            .await
    }

    pub(super) async fn delete_instance_impl(
        &self,
        zone: String,
        instance_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}",
            self.project_id, zone, instance_name
        );
        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &instance_name,
            )
            .await
    }

    pub(super) async fn attach_disk_impl(
        &self,
        zone: String,
        instance_name: String,
        attached_disk: AttachedDisk,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}/attachDisk",
            self.project_id, zone, instance_name
        );
        self.base
            .execute_request(
                Method::POST,
                &path,
                None,
                Some(attached_disk),
                &instance_name,
            )
            .await
    }

    pub(super) async fn detach_disk_impl(
        &self,
        zone: String,
        instance_name: String,
        device_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}/detachDisk",
            self.project_id, zone, instance_name
        );
        let query = vec![("deviceName", device_name)];
        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query),
                Option::<()>::None,
                &instance_name,
            )
            .await
    }

    // --- Disk Operations ---

    pub(super) async fn get_disk_impl(&self, zone: String, disk_name: String) -> Result<Disk> {
        let path = format!(
            "projects/{}/zones/{}/disks/{}",
            self.project_id, zone, disk_name
        );
        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &disk_name)
            .await
    }

    pub(super) async fn insert_disk_impl(&self, zone: String, disk: Disk) -> Result<Operation> {
        let path = format!("projects/{}/zones/{}/disks", self.project_id, zone);
        let resource_name = disk.name.clone().unwrap_or_default();
        self.base
            .execute_request(Method::POST, &path, None, Some(disk), &resource_name)
            .await
    }

    pub(super) async fn delete_disk_impl(
        &self,
        zone: String,
        disk_name: String,
    ) -> Result<Operation> {
        let path = format!(
            "projects/{}/zones/{}/disks/{}",
            self.project_id, zone, disk_name
        );
        self.base
            .execute_request(Method::DELETE, &path, None, Option::<()>::None, &disk_name)
            .await
    }

    pub(super) async fn get_serial_port_output_impl(
        &self,
        zone: String,
        instance_name: String,
    ) -> Result<SerialPortOutput> {
        let path = format!(
            "projects/{}/zones/{}/instances/{}/serialPort",
            self.project_id, zone, instance_name
        );
        self.base
            .execute_request(
                Method::GET,
                &path,
                Some(vec![("port", "1".to_string())]),
                Option::<()>::None,
                &instance_name,
            )
            .await
    }
}
