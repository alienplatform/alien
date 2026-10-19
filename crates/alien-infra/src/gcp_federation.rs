//! Setup-owned customer federation shared by management and published access.

use std::collections::HashMap;

use alien_core::{GcpFederationAccess, GcpFederationTrust, ResourceType};
use alien_error::{AlienError, Context, ContextError};
use alien_gcp_clients::iam::{
    IamApi, WorkloadIdentityOidc, WorkloadIdentityPool, WorkloadIdentityPoolProvider,
};
use alien_gcp_clients::longrunning::{Operation, OperationResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::ResourceControllerContext;
use crate::{ErrorData, Result};

/// Operation checkpoints live inside the existing setup-owned controller.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GcpFederationSetup {
    pending_operation: Option<String>,
    pool_id: Option<String>,
    ownership_marker: Option<String>,
}

impl GcpFederationSetup {
    pub(crate) fn validate_installation(
        ctx: &ResourceControllerContext<'_>,
        purpose: &ResourceType,
    ) -> Result<GcpFederationTrust> {
        let trust = ctx.get_gcp_management_config()?.and_then(|config| config.federation_trust)
            .ok_or_else(|| AlienError::new(alien_core::ErrorData::GcpFederationInvalid {
                field: "managementConfig.federationTrust".to_string(),
                reason: "the selected manager does not advertise federation support; regenerate setup after updating the manager".to_string(),
            }).context(problem("GCP setup requires manager federation support")))?;
        trust
            .validate()
            .context(problem("Invalid manager federation trust"))?;
        let scope = ctx
            .deployment_config
            .gcp_federation_scope
            .as_ref()
            .ok_or_else(|| {
                AlienError::new(alien_core::ErrorData::GcpFederationInvalid {
                    field: "gcpFederationScope".to_string(),
                    reason: "a registered logical installation is required; regenerate setup"
                        .to_string(),
                })
                .context(problem(
                    "GCP setup requires a registered installation scope",
                ))
            })?;
        scope
            .assertion_audience(purpose)
            .context(problem("Invalid installation federation scope"))?;
        Ok(trust)
    }

    pub(crate) fn from_access(access: &GcpFederationAccess) -> alien_core::Result<Self> {
        access.validate()?;
        Ok(Self {
            pending_operation: None,
            pool_id: Some(access.pool_id.clone()),
            ownership_marker: Some(format!(
                "Deployment identity pool. Scope: {}",
                access.assertion_audience
            )),
        })
    }

    /// Makes at most one cloud mutation per call. The caller persists this
    /// checkpoint and delays before advancing the same controller handler.
    pub(crate) async fn reconcile(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        service_account_email: &str,
        purpose: &ResourceType,
    ) -> Result<Option<GcpFederationAccess>> {
        let trust = Self::validate_installation(ctx, purpose)?;
        let scope = ctx
            .deployment_config
            .gcp_federation_scope
            .as_ref()
            .expect("validated installation scope");
        let assertion_audience = scope
            .assertion_audience(purpose)
            .context(problem("Invalid installation federation scope"))?;
        let digest = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}", ctx.resource_prefix, purpose.as_ref()).as_bytes())
        );
        let pool_id = format!("deployment-{}", &digest[..20]);
        let marker = format!("Deployment identity pool. Scope: {assertion_audience}");
        if self.pool_id.as_ref().is_some_and(|old| old != &pool_id)
            || self
                .ownership_marker
                .as_ref()
                .is_some_and(|old| old != &marker)
        {
            return Err(AlienError::new(problem(
                "Federation setup cannot change the installation owning an existing identity pool",
            )));
        }
        self.pool_id = Some(pool_id.clone());
        self.ownership_marker = Some(marker.clone());
        let client = ctx
            .service_provider
            .get_gcp_iam_client(ctx.get_gcp_config()?)?;
        if !self.poll_pending(&*client).await? {
            return Ok(None);
        }
        let pool = match client.get_workload_identity_pool(pool_id.clone()).await {
            Ok(pool) => pool,
            Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => {
                let pool = WorkloadIdentityPool::builder()
                    .display_name("Deployment identity pool".to_string())
                    .description(marker.clone())
                    .build();
                self.record(
                    client
                        .create_workload_identity_pool(pool_id, pool)
                        .await
                        .context(problem("Cannot create deployment identity pool"))?,
                )?;
                return Ok(None);
            }
            Err(error) => {
                return Err(error.context(problem("Cannot inspect deployment identity pool")))
            }
        };
        if pool.description.as_deref() != Some(marker.as_str()) {
            return Err(AlienError::new(problem(
                "Identity pool does not belong to this installation; refusing to change its trust",
            )));
        }
        if pool.state.as_deref() == Some("DELETED") {
            self.record(
                client
                    .undelete_workload_identity_pool(pool_id)
                    .await
                    .context(problem("Cannot restore deployment identity pool"))?,
            )?;
            return Ok(None);
        }
        if pool.state.as_deref() != Some("ACTIVE") {
            return Err(AlienError::new(problem(
                "Deployment identity pool has an unsupported lifecycle state",
            )));
        }
        if pool.disabled {
            self.record(
                client
                    .update_workload_identity_pool(pool_id, WorkloadIdentityPool::default())
                    .await
                    .context(problem("Cannot enable deployment identity pool"))?,
            )?;
            return Ok(None);
        }
        let canonical = pool.name.as_deref().ok_or_else(|| {
            AlienError::new(problem(
                "Google did not return the identity pool's canonical name",
            ))
        })?;
        let suffix = format!("/locations/global/workloadIdentityPools/{pool_id}");
        let number = canonical
            .strip_prefix("projects/")
            .and_then(|name| name.strip_suffix(&suffix))
            .ok_or_else(|| {
                AlienError::new(problem("Google returned an unexpected identity pool name"))
            })?;
        let access = GcpFederationAccess {
            project_number: number.to_string(),
            pool_id: pool_id.clone(),
            provider_id: "manager".to_string(),
            service_account_email: service_account_email.to_string(),
            assertion_audience,
            trust: trust.clone(),
        };
        access.validate().context(problem(
            "Google returned invalid deployment federation access",
        ))?;
        let desired = WorkloadIdentityPoolProvider::builder()
            .display_name("Deployment manager".to_string())
            .attribute_mapping(HashMap::from([(
                "google.subject".to_string(),
                "assertion.sub".to_string(),
            )]))
            .attribute_condition(trust.subject_condition())
            .oidc(
                WorkloadIdentityOidc::builder()
                    .issuer_uri(trust.issuer.clone())
                    .allowed_audiences(vec![access.assertion_audience.clone()])
                    .build(),
            )
            .build();
        let provider = match client
            .get_workload_identity_provider(pool_id.clone(), access.provider_id.clone())
            .await
        {
            Ok(provider) => provider,
            Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => {
                self.record(
                    client
                        .create_workload_identity_provider(
                            pool_id,
                            access.provider_id.clone(),
                            desired,
                        )
                        .await
                        .context(problem("Cannot create deployment identity provider"))?,
                )?;
                return Ok(None);
            }
            Err(error) => {
                return Err(error.context(problem("Cannot inspect deployment identity provider")))
            }
        };
        if provider.state.as_deref() == Some("DELETED") {
            self.record(
                client
                    .undelete_workload_identity_provider(pool_id, access.provider_id.clone())
                    .await
                    .context(problem("Cannot restore deployment identity provider"))?,
            )?;
            return Ok(None);
        }
        if provider.state.as_deref() != Some("ACTIVE") {
            return Err(AlienError::new(problem(
                "Deployment identity provider has an unsupported lifecycle state",
            )));
        }
        if provider.disabled
            || provider.oidc != desired.oidc
            || provider.attribute_mapping != desired.attribute_mapping
            || provider.attribute_condition != desired.attribute_condition
        {
            self.record(
                client
                    .update_workload_identity_provider(pool_id, access.provider_id.clone(), desired)
                    .await
                    .context(problem(
                        "Cannot reconcile deployment identity provider trust",
                    ))?,
            )?;
            return Ok(None);
        }
        Ok(Some(access))
    }

    pub(crate) async fn delete(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        let Some(pool_id) = self.pool_id.clone() else {
            return Ok(true);
        };
        let client = ctx
            .service_provider
            .get_gcp_iam_client(ctx.get_gcp_config()?)?;
        if !self.poll_pending(&*client).await? {
            return Ok(false);
        }
        let pool = match client.get_workload_identity_pool(pool_id.clone()).await {
            Ok(pool) => pool,
            Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => return Ok(true),
            Err(error) => {
                return Err(error.context(problem("Cannot inspect identity pool during deletion")))
            }
        };
        if pool.description.as_ref() != self.ownership_marker.as_ref() {
            return Err(AlienError::new(problem(
                "Refusing to delete an identity pool with a different installation owner",
            )));
        }
        if pool.state.as_deref() == Some("DELETED") {
            return Ok(true);
        }
        // Deleting a pool disables all its providers; its name stays reserved
        // during Google's recovery period. A setup retry restores the same pool.
        self.record(
            client
                .delete_workload_identity_pool(pool_id)
                .await
                .context(problem("Cannot delete deployment identity pool"))?,
        )?;
        Ok(false)
    }

    async fn poll_pending(&mut self, client: &dyn IamApi) -> Result<bool> {
        let Some(name) = self.pending_operation.clone() else {
            return Ok(true);
        };
        let operation = match client.get_workload_identity_operation(name).await {
            Ok(operation) => operation,
            Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => {
                // Expired operation handles are recoverable by inspecting the
                // owned resources. Other errors must keep the checkpoint.
                self.pending_operation = None;
                return Ok(true);
            }
            Err(error) => {
                return Err(
                    error.context(problem("Cannot inspect pending identity setup operation"))
                )
            }
        };
        if operation.done != Some(true) {
            return Ok(false);
        }
        // Clear a completed failed operation so an authorized retry can repair
        // the observed resources instead of polling the same failure forever.
        self.pending_operation = None;
        operation_succeeded(&operation)?;
        Ok(true)
    }

    fn record(&mut self, operation: Operation) -> Result<()> {
        if operation.done == Some(true) {
            return operation_succeeded(&operation);
        }
        self.pending_operation = Some(operation.name.ok_or_else(|| {
            AlienError::new(problem(
                "Identity setup mutation returned no operation handle",
            ))
        })?);
        Ok(())
    }
}

fn operation_succeeded(operation: &Operation) -> Result<()> {
    if let Some(OperationResult::Error { error }) = &operation.result {
        return Err(
            AlienError::new(alien_client_core::ErrorData::RemoteOperationFailed {
                operation: "Identity setup".to_string(),
                resource_name: operation
                    .name
                    .clone()
                    .unwrap_or_else(|| "deployment identity pool".to_string()),
                status: "FAILED".to_string(),
                provider_code: Some(error.code.to_string()),
                message: error.message.clone(),
            })
            .context(problem("Deployment identity operation failed")),
        );
    }
    Ok(())
}

fn problem(message: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        message: message.to_string(),
        resource_id: None,
    }
}
