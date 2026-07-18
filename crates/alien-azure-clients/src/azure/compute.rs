use crate::azure::common::{AzureClientBase, AzureRequestBuilder};
use crate::azure::long_running_operation::OperationResult;
use crate::azure::models::compute_rp::{
    CachingTypes, DataDisk, DiskCreateOptionTypes, ManagedDiskParameters,
    RetrieveBootDiagnosticsDataResult, RunCommandInput, RunCommandResult, VirtualMachineScaleSet,
    VirtualMachineScaleSetVm, VirtualMachineScaleSetVmListResult,
};
use crate::azure::token_cache::AzureTokenCache;
use alien_client_core::{ErrorData, Result};

use alien_error::{Context, IntoAlienError};
use reqwest::{Client, Method};

#[cfg(feature = "test-utils")]
use mockall::automock;

/// Result of a VMSS create or update operation
pub type VmssOperationResult = OperationResult<VirtualMachineScaleSet>;

// -------------------------------------------------------------------------
// Azure Virtual Machine Scale Sets API trait
// -------------------------------------------------------------------------

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait VirtualMachineScaleSetsApi: Send + Sync + std::fmt::Debug {
    // -------------------------------------------------------------------------
    // Virtual Machine Scale Set Operations
    // -------------------------------------------------------------------------

    /// Create or update a virtual machine scale set
    ///
    /// This method handles the Azure VMSS API for both creating new scale sets
    /// and updating existing ones. Azure uses PUT semantics for both operations.
    ///
    /// The operation may complete synchronously (201/200 with result) or be long-running
    /// (202 with polling URLs). Use the returned OperationResult to handle both cases.
    async fn create_or_update_vmss(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        vmss: &VirtualMachineScaleSet,
    ) -> Result<VmssOperationResult>;

    /// Get a virtual machine scale set by name
    async fn get_vmss(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<VirtualMachineScaleSet>;

    /// Delete a virtual machine scale set
    ///
    /// This method deletes a Virtual Machine Scale Set. The operation may complete
    /// synchronously with a 204 status code if the deletion is immediate, or
    /// asynchronously returning a 202 status code if the deletion is in progress.
    async fn delete_vmss(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<OperationResult<()>>;

    // -------------------------------------------------------------------------
    // Virtual Machine Scale Set VM Operations
    // -------------------------------------------------------------------------

    /// List all VMs in a virtual machine scale set, following every nextLink page.
    async fn list_vmss_vms(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<VirtualMachineScaleSetVmListResult>;

    /// Get a specific VM instance in a virtual machine scale set
    async fn get_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
    ) -> Result<VirtualMachineScaleSetVm>;

    /// Update the base64-encoded userData exposed to one VMSS VM instance.
    ///
    /// Azure treats this PUT as a long-running operation. The caller owns
    /// encoding the UTF-8 payload and must not put secrets in userData.
    async fn update_vmss_vm_user_data(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
        location: &str,
        user_data_base64: &str,
    ) -> Result<OperationResult<VirtualMachineScaleSetVm>>;

    /// Delete a specific VM instance in a virtual machine scale set
    async fn delete_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
    ) -> Result<OperationResult<()>>;

    /// Run a command on a specific VM instance in a virtual machine scale set
    ///
    /// This operation runs a command on a VM instance and returns the result.
    /// The operation is typically long-running.
    async fn run_command_on_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
        command: &RunCommandInput,
    ) -> Result<OperationResult<RunCommandResult>>;

    // -------------------------------------------------------------------------
    // Disk Attachment Operations
    // -------------------------------------------------------------------------

    /// Attach a managed disk to a VMSS VM instance
    ///
    /// This operation attaches a managed disk to a virtual machine in a scale set.
    /// The disk must already exist and be in the same region as the VM.
    /// The operation is typically long-running.
    async fn attach_disk_to_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
        disk_id: &str,
        lun: i32,
    ) -> Result<OperationResult<VirtualMachineScaleSetVm>>;

    /// Detach a managed disk from a VMSS VM instance
    ///
    /// This operation detaches a managed disk from a virtual machine in a scale set.
    /// The operation is typically long-running.
    async fn detach_disk_from_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
        lun: i32,
    ) -> Result<OperationResult<VirtualMachineScaleSetVm>>;

    // -------------------------------------------------------------------------
    // Boot Diagnostics Operations
    // -------------------------------------------------------------------------

    /// Retrieve the serial console log for a VMSS VM instance.
    ///
    /// Calls the `retrieveBootDiagnosticsData` API to obtain a SAS URL for the
    /// serial console log, then fetches and returns the log content.
    /// Boot diagnostics must be enabled on the VMSS for this to work.
    ///
    /// See: https://learn.microsoft.com/en-us/rest/api/compute/virtual-machine-scale-set-vms/retrieve-boot-diagnostics-data
    async fn get_vmss_vm_serial_console_log(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
    ) -> Result<String>;

    // -------------------------------------------------------------------------
    // Rolling Upgrade Operations
    // -------------------------------------------------------------------------

    /// Start an OS rolling upgrade for a VMSS.
    /// See: https://learn.microsoft.com/en-us/rest/api/compute/virtual-machine-scale-set-rolling-upgrades/start-os-upgrade
    async fn start_vmss_rolling_upgrade(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<OperationResult<()>>;

    /// Get the status of the latest rolling upgrade for a VMSS.
    /// See: https://learn.microsoft.com/en-us/rest/api/compute/virtual-machine-scale-set-rolling-upgrades/get-latest
    async fn get_vmss_rolling_upgrade_latest(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<RollingUpgradeLatestStatus>;
}

// -------------------------------------------------------------------------
// Azure Virtual Machine Scale Sets client struct
// -------------------------------------------------------------------------

/// Azure Virtual Machine Scale Sets client for managing VMSS and their VM instances.
#[derive(Debug)]
pub struct AzureVmssClient {
    pub base: AzureClientBase,
    pub token_cache: AzureTokenCache,
}

impl AzureVmssClient {
    /// API version for Azure Compute resources
    const API_VERSION: &'static str = "2024-07-01";

    pub fn new(client: Client, token_cache: AzureTokenCache) -> Self {
        // Azure Resource Manager endpoint
        let endpoint = token_cache.management_endpoint().to_string();

        Self {
            base: AzureClientBase::with_client_config(
                client,
                endpoint,
                token_cache.config().clone(),
            ),
            token_cache,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl VirtualMachineScaleSetsApi for AzureVmssClient {
    // -------------------------------------------------------------------------
    // Virtual Machine Scale Set Operations
    // -------------------------------------------------------------------------

    async fn create_or_update_vmss(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        vmss: &VirtualMachineScaleSet,
    ) -> Result<VmssOperationResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(vmss).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize VMSS: {}", vmss_name),
            },
        )?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "CreateOrUpdateVmss", vmss_name)
            .await
    }

    async fn get_vmss(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<VirtualMachineScaleSet> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetVmss", vmss_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetVmss: failed to read response body for {}",
                    vmss_name
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let vmss: VirtualMachineScaleSet = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!("Azure GetVmss: JSON parse error for {}", vmss_name),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(vmss)
    }

    async fn delete_vmss(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "DeleteVmss", vmss_name)
            .await
    }

    // -------------------------------------------------------------------------
    // Virtual Machine Scale Set VM Operations
    // -------------------------------------------------------------------------

    async fn list_vmss_vms(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<VirtualMachineScaleSetVmListResult> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let mut next_url = Some(self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/virtualMachines", 
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name),
            Some(vec![
                ("api-version", Self::API_VERSION.into()),
                ("$expand", "instanceView".into()),
            ]),
        ));
        let mut vms = Vec::new();

        while let Some(url) = next_url {
            let request = AzureRequestBuilder::new(Method::GET, url.clone())
                .content_length("")
                .build()?;
            let signed = self.base.sign_request(request, &bearer_token).await?;
            let response = self
                .base
                .execute_request(signed, "ListVmssVms", vmss_name)
                .await?;
            let body =
                response
                    .text()
                    .await
                    .into_alien_error()
                    .context(ErrorData::HttpResponseError {
                        message: format!(
                            "Azure ListVmssVms: failed to read response body for {}",
                            vmss_name
                        ),
                        url: url.clone(),
                        http_status: 200,
                        http_response_text: None,
                        http_request_text: None,
                    })?;
            let page: VirtualMachineScaleSetVmListResult = serde_json::from_str(&body)
                .into_alien_error()
                .context(ErrorData::HttpResponseError {
                    message: format!("Azure ListVmssVms: JSON parse error for {}", vmss_name),
                    url,
                    http_status: 200,
                    http_response_text: Some(body.clone()),
                    http_request_text: None,
                })?;

            vms.extend(page.value);
            next_url = page.next_link;
        }

        Ok(VirtualMachineScaleSetVmListResult {
            next_link: None,
            value: vms,
        })
    }

    async fn get_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
    ) -> Result<VirtualMachineScaleSetVm> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/virtualMachines/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name, instance_id),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetVmssVm", instance_id)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetVmssVm: failed to read response body for {}/{}",
                    vmss_name, instance_id
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let vm: VirtualMachineScaleSetVm = serde_json::from_str(&body).into_alien_error().context(
            ErrorData::HttpResponseError {
                message: format!(
                    "Azure GetVmssVm: JSON parse error for {}/{}",
                    vmss_name, instance_id
                ),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            },
        )?;

        Ok(vm)
    }

    async fn update_vmss_vm_user_data(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
        location: &str,
        user_data_base64: &str,
    ) -> Result<OperationResult<VirtualMachineScaleSetVm>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/virtualMachines/{}",
                &self.token_cache.config().subscription_id,
                resource_group_name,
                vmss_name,
                instance_id
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );
        let body = serde_json::to_string(&serde_json::json!({
            "location": location,
            "properties": {
                "userData": user_data_base64,
            },
        }))
        .into_alien_error()
        .context(ErrorData::SerializationError {
            message: format!(
                "Failed to serialize VMSS VM userData update for {}/{}",
                vmss_name, instance_id
            ),
        })?;

        let request = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body)
            .build()?;
        let signed = self.base.sign_request(request, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "UpdateVmssVmUserData", instance_id)
            .await
    }

    async fn delete_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/virtualMachines/{}", 
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name, instance_id),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::DELETE, url).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "DeleteVmssVm", instance_id)
            .await
    }

    async fn run_command_on_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
        command: &RunCommandInput,
    ) -> Result<OperationResult<RunCommandResult>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/virtualMachines/{}/runCommand", 
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name, instance_id),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(command).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize run command for {}/{}",
                    vmss_name, instance_id
                ),
            },
        )?;

        let builder = AzureRequestBuilder::new(Method::POST, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "RunCommandOnVmssVm", instance_id)
            .await
    }

    async fn attach_disk_to_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
        disk_id: &str,
        lun: i32,
    ) -> Result<OperationResult<VirtualMachineScaleSetVm>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        // Get current VM to retrieve existing storage profile
        let mut vm = self
            .get_vmss_vm(resource_group_name, vmss_name, instance_id)
            .await?;

        // Initialize storage profile if it doesn't exist
        if vm
            .properties
            .as_ref()
            .and_then(|p| p.storage_profile.as_ref())
            .is_none()
        {
            if let Some(ref mut props) = vm.properties {
                props.storage_profile = Some(Default::default());
            }
        }

        // Add the new disk to the data disks array
        if let Some(ref mut props) = vm.properties {
            if let Some(ref mut storage_profile) = props.storage_profile {
                storage_profile.data_disks.push(DataDisk {
                    caching: Some(CachingTypes::None),
                    create_option: DiskCreateOptionTypes::Attach,
                    delete_option: None,
                    detach_option: None,
                    disk_iops_read_write: None,
                    disk_m_bps_read_write: None,
                    disk_size_gb: None,
                    image: None,
                    lun,
                    managed_disk: Some(ManagedDiskParameters {
                        id: Some(disk_id.to_string()),
                        ..Default::default()
                    }),
                    name: None,
                    source_resource: None,
                    to_be_detached: None,
                    vhd: None,
                    write_accelerator_enabled: None,
                });
            }
        }

        // Update the VM with the new disk attached
        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/virtualMachines/{}",
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name, instance_id),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(&vm).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize VMSS VM for disk attachment: {}/{}",
                    vmss_name, instance_id
                ),
            },
        )?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "AttachDiskToVmssVm", instance_id)
            .await
    }

    async fn detach_disk_from_vmss_vm(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
        lun: i32,
    ) -> Result<OperationResult<VirtualMachineScaleSetVm>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        // Get current VM to retrieve existing storage profile
        let mut vm = self
            .get_vmss_vm(resource_group_name, vmss_name, instance_id)
            .await?;

        // Remove the disk with the specified LUN
        if let Some(ref mut props) = vm.properties {
            if let Some(ref mut storage_profile) = props.storage_profile {
                storage_profile.data_disks.retain(|disk| disk.lun != lun);
            }
        }

        // Update the VM with the disk removed
        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/virtualMachines/{}",
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name, instance_id),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let body = serde_json::to_string(&vm).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize VMSS VM for disk detachment: {}/{}",
                    vmss_name, instance_id
                ),
            },
        )?;

        let builder = AzureRequestBuilder::new(Method::PUT, url)
            .content_type_json()
            .content_length(&body)
            .body(body);

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;

        self.base
            .execute_request_with_long_running_support(signed, "DetachDiskFromVmssVm", instance_id)
            .await
    }

    async fn get_vmss_vm_serial_console_log(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
        instance_id: &str,
    ) -> Result<String> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;

        // Step 1: Call retrieveBootDiagnosticsData to get SAS URLs for the boot logs.
        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/virtualMachines/{}/retrieveBootDiagnosticsData",
                &self.token_cache.config().subscription_id, resource_group_name, vmss_name, instance_id
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::POST, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "RetrieveBootDiagnosticsData", vmss_name)
            .await?;

        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Failed to read retrieveBootDiagnosticsData response for {}/{}",
                    vmss_name, instance_id
                ),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;

        let diagnostics: RetrieveBootDiagnosticsDataResult = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!(
                    "Failed to parse retrieveBootDiagnosticsData response for {}/{}",
                    vmss_name, instance_id
                ),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            })?;

        // Step 2: Fetch the serial console log from the SAS URL (unauthenticated blob fetch).
        let log_uri = match diagnostics.serial_console_log_blob_uri {
            Some(uri) if !uri.is_empty() => uri,
            _ => return Ok(String::new()),
        };

        let log_content = self
            .base
            .client
            .get(&log_uri)
            .send()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!(
                    "Failed to fetch serial console log for {}/{}",
                    vmss_name, instance_id
                ),
            })?
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpRequestFailed {
                message: format!(
                    "Failed to read serial console log for {}/{}",
                    vmss_name, instance_id
                ),
            })?;

        Ok(log_content)
    }

    async fn start_vmss_rolling_upgrade(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<OperationResult<()>> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/osRollingUpgrade",
                &self.token_cache.config().subscription_id, resource_group_name, vmss_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );
        let builder = AzureRequestBuilder::new(Method::POST, url).content_length("");
        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        self.base
            .execute_request_with_long_running_support(signed, "StartVmssRollingUpgrade", vmss_name)
            .await
    }

    async fn get_vmss_rolling_upgrade_latest(
        &self,
        resource_group_name: &str,
        vmss_name: &str,
    ) -> Result<RollingUpgradeLatestStatus> {
        let bearer_token = self
            .token_cache
            .get_bearer_token_with_scope("https://management.azure.com/.default")
            .await?;
        let url = self.base.build_url(
            &format!(
                "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/rollingUpgrades/latest",
                &self.token_cache.config().subscription_id, resource_group_name, vmss_name
            ),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );
        let builder = AzureRequestBuilder::new(Method::GET, url.clone());
        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "GetVmssRollingUpgradeLatest", vmss_name)
            .await?;
        let body = resp
            .text()
            .await
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!("Failed to read rolling upgrade status for {}", vmss_name),
                url: url.clone(),
                http_status: 200,
                http_response_text: None,
                http_request_text: None,
            })?;
        serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!("Failed to parse rolling upgrade status for {}", vmss_name),
                url,
                http_status: 200,
                http_response_text: Some(body),
                http_request_text: None,
            })
    }
}

// -------------------------------------------------------------------------
// Rolling Upgrade Status Types
// -------------------------------------------------------------------------

use serde::Deserialize;

/// Status of the latest rolling upgrade for a VMSS.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RollingUpgradeLatestStatus {
    pub properties: Option<RollingUpgradeStatusProperties>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RollingUpgradeStatusProperties {
    #[serde(rename = "runningStatus")]
    pub running_status: Option<RollingUpgradeRunningStatus>,
    pub progress: Option<RollingUpgradeProgressInfo>,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RollingUpgradeRunningStatus {
    /// Code: RollingForward, Cancelled, Completed, Faulted
    pub code: Option<String>,
    #[serde(rename = "startTime")]
    pub start_time: Option<String>,
    #[serde(rename = "lastAction")]
    pub last_action: Option<String>,
    #[serde(rename = "lastActionTime")]
    pub last_action_time: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RollingUpgradeProgressInfo {
    #[serde(rename = "successfulInstanceCount")]
    pub successful_instance_count: Option<i32>,
    #[serde(rename = "failedInstanceCount")]
    pub failed_instance_count: Option<i32>,
    #[serde(rename = "inProgressInstanceCount")]
    pub in_progress_instance_count: Option<i32>,
    #[serde(rename = "pendingInstanceCount")]
    pub pending_instance_count: Option<i32>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use httpmock::{Method::GET, Method::PUT, MockServer};
    use serde_json::json;

    use super::*;
    use crate::azure::{AzureClientConfig, AzureClientConfigExt, ServiceOverrides};

    const SUBSCRIPTION_ID: &str = "12345678-1234-1234-1234-123456789012";
    const USER_DATA_BASE64: &str = "eyJ2ZXJzaW9uIjoxLCJob3Jpem9uSWRlbnRpdHlDbGllbnRJZCI6IjExMTExMTExLTIyMjItMzMzMy00NDQ0LTU1NTU1NTU1NTU1NSJ9";

    fn test_client(server: &MockServer) -> AzureVmssClient {
        let config = AzureClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("management".to_string(), server.base_url())]),
        });
        AzureVmssClient::new(Client::new(), AzureTokenCache::new(config))
    }

    fn vm_path() -> String {
        format!(
            "/subscriptions/{SUBSCRIPTION_ID}/resourceGroups/test-rg/providers/Microsoft.Compute/virtualMachineScaleSets/test-vmss/virtualMachines/3"
        )
    }

    #[tokio::test]
    async fn lists_every_vmss_vm_page() {
        let server = MockServer::start_async().await;
        let next_link = format!("{}/vm-pages/2?continuation=next", server.base_url());
        let first_page = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(format!(
                        "/subscriptions/{SUBSCRIPTION_ID}/resourceGroups/test-rg/providers/Microsoft.Compute/virtualMachineScaleSets/test-vmss/virtualMachines"
                    ))
                    .query_param("api-version", "2024-07-01")
                    .query_param("$expand", "instanceView");
                then.status(200).json_body(json!({
                    "value": [{
                        "location": "eastus",
                        "instanceId": "3"
                    }],
                    "nextLink": next_link,
                }));
            })
            .await;
        let second_page = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path("/vm-pages/2")
                    .query_param("continuation", "next");
                then.status(200).json_body(json!({
                    "value": [{
                        "location": "eastus",
                        "instanceId": "7"
                    }]
                }));
            })
            .await;

        let result = test_client(&server)
            .list_vmss_vms("test-rg", "test-vmss")
            .await
            .expect("all VM pages should be returned");

        first_page.assert_async().await;
        second_page.assert_async().await;
        assert!(result.next_link.is_none());
        assert_eq!(
            result
                .value
                .iter()
                .filter_map(|vm| vm.instance_id.as_deref())
                .collect::<Vec<_>>(),
            vec!["3", "7"]
        );
    }

    #[tokio::test]
    async fn updates_vmss_vm_user_data_with_minimal_encoded_request() {
        let server = MockServer::start_async().await;
        let update = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(vm_path())
                    .query_param("api-version", "2024-07-01")
                    .json_body(json!({
                        "location": "eastus",
                        "properties": {
                            "userData": USER_DATA_BASE64,
                        },
                    }));
                then.status(200).json_body(json!({
                    "location": "eastus",
                    "properties": {
                        "userData": USER_DATA_BASE64,
                    },
                }));
            })
            .await;

        let result = test_client(&server)
            .update_vmss_vm_user_data("test-rg", "test-vmss", "3", "eastus", USER_DATA_BASE64)
            .await
            .expect("userData update should succeed");

        update.assert_async().await;
        let OperationResult::Completed(vm) = result else {
            panic!("expected synchronous VM update");
        };
        assert_eq!(
            vm.properties.and_then(|properties| properties.user_data),
            Some(USER_DATA_BASE64.to_string())
        );
    }

    #[tokio::test]
    async fn returns_vmss_vm_user_data_long_running_operation() {
        let server = MockServer::start_async().await;
        let operation_url = format!("{}/operations/user-data-update", server.base_url());
        let update = server
            .mock_async(|when, then| {
                when.method(PUT)
                    .path(vm_path())
                    .query_param("api-version", "2024-07-01");
                then.status(202)
                    .header("Azure-AsyncOperation", &operation_url)
                    .header("Retry-After", "7");
            })
            .await;

        let result = test_client(&server)
            .update_vmss_vm_user_data("test-rg", "test-vmss", "3", "eastus", USER_DATA_BASE64)
            .await
            .expect("accepted userData update should return an LRO");

        update.assert_async().await;
        let OperationResult::LongRunning(operation) = result else {
            panic!("expected asynchronous VM update");
        };
        assert_eq!(operation.url, operation_url);
        assert_eq!(operation.retry_after, Some(Duration::from_secs(7)));
    }
}
