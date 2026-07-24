use super::*;

impl GcpWorkerController {
    pub(super) fn record_compute_operation(
        &mut self,
        operation: ComputeOperation,
        region: Option<String>,
        resource_id: &str,
        operation_label: &str,
    ) -> Result<()> {
        if operation.has_error() {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation_label} failed: {error_msg}"),
                resource_id: Some(resource_id.to_string()),
            }));
        }

        if operation.is_done() {
            self.compute_operation_name = None;
            self.compute_operation_region = None;
            return Ok(());
        }

        let operation_name = operation.name.ok_or_else(|| {
            AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation_label} returned without operation name"),
                resource_id: Some(resource_id.to_string()),
            })
        })?;

        self.compute_operation_name = Some(operation_name);
        self.compute_operation_region = region;
        Ok(())
    }

    pub(super) async fn compute_operation_done(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        operation_label: &str,
    ) -> Result<bool> {
        let Some(operation_name) = self.compute_operation_name.as_ref() else {
            return Ok(true);
        };

        let gcp_config = ctx.get_gcp_config()?;
        let compute_client = ctx.service_provider.get_gcp_compute_client(gcp_config)?;

        let operation = if let Some(region) = &self.compute_operation_region {
            compute_client
                .get_region_operation(region.clone(), operation_name.clone())
                .await
        } else {
            compute_client
                .get_global_operation(operation_name.clone())
                .await
        }
        .context(ErrorData::CloudPlatformError {
            message: format!("Failed to check {operation_label} status"),
            resource_id: Some(resource_id.to_string()),
        })?;

        if !operation.is_done() {
            debug!(
                operation_name=%operation_name,
                operation=%operation_label,
                "Compute operation still in progress"
            );
            return Ok(false);
        }

        if operation.has_error() {
            let error_msg = operation
                .error
                .and_then(|e| e.errors.first().and_then(|err| err.message.clone()))
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(AlienError::new(ErrorData::CloudPlatformError {
                message: format!("{operation_label} failed: {error_msg}"),
                resource_id: Some(resource_id.to_string()),
            }));
        }

        self.compute_operation_name = None;
        self.compute_operation_region = None;
        Ok(true)
    }
}
