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

    /// List all VMs in a virtual machine scale set
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

        let url = self.base.build_url(
            &format!("/subscriptions/{}/resourceGroups/{}/providers/Microsoft.Compute/virtualMachineScaleSets/{}/virtualMachines", 
                     &self.token_cache.config().subscription_id, resource_group_name, vmss_name),
            Some(vec![("api-version", Self::API_VERSION.into())]),
        );

        let builder = AzureRequestBuilder::new(Method::GET, url.clone()).content_length("");

        let req = builder.build()?;
        let signed = self.base.sign_request(req, &bearer_token).await?;
        let resp = self
            .base
            .execute_request(signed, "ListVmssVms", vmss_name)
            .await?;

        let body = resp
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

        let result: VirtualMachineScaleSetVmListResult = serde_json::from_str(&body)
            .into_alien_error()
            .context(ErrorData::HttpResponseError {
                message: format!("Azure ListVmssVms: JSON parse error for {}", vmss_name),
                url,
                http_status: 200,
                http_response_text: Some(body.clone()),
                http_request_text: None,
            })?;

        Ok(result)
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
