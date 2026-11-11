use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use alien_azure_clients::{
    authorization::Scope,
    keyvault::{CreateKeyAttributes, CreateKeyRequest, KeyVaultKeysApi},
    models::{
        authorization_role_assignments::{
            RoleAssignment, RoleAssignmentProperties, RoleAssignmentPropertiesPrincipalType,
        },
        keyvault::{Sku, SkuFamily, SkuName, VaultCreateOrUpdateParameters, VaultProperties},
        resources::ResourceGroup,
    },
};
use alien_core::{
    bindings::KeyBinding,
    import::{data::AzureKeyImportData, ImportContext},
    AzureKeyVaultKeyHeartbeatData, HeartbeatBackend, Key, KeyFingerprint, KeyHeartbeatData,
    KeyHeartbeatStatus, KeyOutputs, ObservedHealth, Platform, ProviderLifecycleState,
    ResourceHeartbeat, ResourceHeartbeatData, ResourceOutputs, ResourceStatus, StackResourceState,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_macros::controller;
use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    core::{AzurePermissionsHelper, ResourceControllerContext, ResourcePermissionsHelper},
    error::{ErrorData, Result},
    import::ResourceImporter,
    import_helpers::make_imported_state,
};

/// `Key Vault Crypto Officer`: lets the setup identity create the key.
const CRYPTO_OFFICER_ROLE: &str = "14b46e9e-c2b7-41b4-b07b-48a6ebf60603";
const KEY_NAME: &str = "key";
/// RBAC takes a while to reach the data plane; the create is retried about this many times.
const MAX_KEY_CREATE_ATTEMPTS: u32 = 40;

#[controller]
pub struct AzureKeyController {
    #[serde(default)]
    pub(crate) available: Option<bool>,
    pub(crate) vault_resource_id: Option<String>,
    pub(crate) key_name: Option<String>,
    pub(crate) lineage_version_id: Option<String>,
    pub(crate) key_id: Option<String>,
    /// Whether Alien made this key in a direct setup run. Older states are imports.
    #[serde(default)]
    pub(crate) origin: super::KeyOrigin,
    /// The vault's name, chosen before the first call so every retry reuses it.
    #[serde(default)]
    pub(crate) vault_name: Option<String>,
    /// The `<prefix>-keys` group that holds the vault. Destroy never deletes it.
    #[serde(default)]
    pub(crate) resource_group_name: Option<String>,
    #[serde(default)]
    pub(crate) vault_uri: Option<String>,
    /// Full id of the role assignment that lets setup create the key.
    #[serde(default)]
    pub(crate) installer_assignment_id: Option<String>,
    #[serde(default)]
    pub(crate) key_create_attempts: u32,
    /// Digest of the grants last written.
    #[serde(default)]
    pub(crate) access_revision: Option<String>,
    /// Ids of the role assignments the grants made, at the key's scope.
    #[serde(default)]
    pub(crate) role_assignment_ids: Vec<String>,
}

#[controller]
impl AzureKeyController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        self.vault_name = Some(vault_name(ctx.resource_prefix, &key.id));
        self.resource_group_name = Some(keys_group_name(ctx.resource_prefix));
        Ok(HandlerAction::Continue {
            state: EnsuringKeysResourceGroup,
            suggested_delay: None,
        })
    }

    /// The vault lives in its own group, not the deployment's: erase deletes the deployment's
    /// group, and the key must outlive it.
    #[handler(state = EnsuringKeysResourceGroup, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn ensuring_keys_resource_group(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let azure = ctx.get_azure_config()?;
        let group = self
            .resource_group_name
            .clone()
            .ok_or_else(|| missing(&key.id, "resource group name"))?;
        let client = ctx.service_provider.get_azure_resources_client(azure)?;
        client
            .create_or_update_resource_group(
                &group,
                &ResourceGroup {
                    id: None,
                    location: azure.region.clone().unwrap_or_else(|| "East US".to_string()),
                    managed_by: None,
                    name: Some(group.clone()),
                    properties: None,
                    tags: resource_tags(ctx.resource_prefix, &key.id),
                    type_: None,
                },
            )
            .await
            .context(ErrorData::CloudPlatformError {
                message: format!("Failed to create the key resource group '{group}'"),
                resource_id: Some(key.id.clone()),
            })?;
        Ok(HandlerAction::Continue {
            state: CreatingVault,
            suggested_delay: None,
        })
    }

    #[handler(state = CreatingVault, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_vault(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let azure = ctx.get_azure_config()?;
        let (group, name) = self.group_and_vault(&key.id)?;
        let tenant_id = Uuid::parse_str(&azure.tenant_id)
            .into_alien_error()
            .context(ErrorData::ResourceConfigInvalid {
                resource_id: Some(key.id.clone()),
                message: format!("Invalid Azure tenant id '{}'", azure.tenant_id),
            })?;
        let client = ctx
            .service_provider
            .get_azure_key_vault_management_client(azure)?;
        let tags = resource_tags(ctx.resource_prefix, &key.id);
        // A vault under our name is used only when it is what Alien creates: the name is
        // global, so it may be someone else's, and tags are writable by anyone.
        match client.get_vault(group.clone(), name.clone()).await {
            Ok(existing) => {
                check_vault_adoptable(&existing, &tenant_id, &tags, &name, &key.id)?;
                self.vault_uri = existing.properties.vault_uri.clone();
                self.vault_resource_id = existing.id;
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                let created = client
                    .create_or_update_vault(
                        group.clone(),
                        name.clone(),
                        VaultCreateOrUpdateParameters {
                            location: azure
                                .region
                                .clone()
                                .unwrap_or_else(|| "East US".to_string()),
                            properties: VaultProperties {
                                access_policies: vec![],
                                create_mode: None,
                                enable_purge_protection: Some(true),
                                enable_rbac_authorization: true,
                                enable_soft_delete: true,
                                enabled_for_deployment: false,
                                enabled_for_disk_encryption: false,
                                enabled_for_template_deployment: false,
                                hsm_pool_resource_id: None,
                                network_acls: None,
                                private_endpoint_connections: vec![],
                                provisioning_state: None,
                                public_network_access: "Enabled".to_string(),
                                sku: Sku {
                                    name: SkuName::Standard,
                                    family: SkuFamily::A,
                                },
                                soft_delete_retention_in_days: 90,
                                tenant_id,
                                vault_uri: None,
                            },
                            tags: tags.clone(),
                        },
                    )
                    .await
                    .context(ErrorData::CloudPlatformError {
                        message: format!("Failed to create the Key Vault '{name}'"),
                        resource_id: Some(key.id.clone()),
                    })?;
                self.vault_uri = created.properties.vault_uri.clone();
                self.vault_resource_id = created.id;
            }
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: format!("Failed to read the Key Vault '{name}'"),
                    resource_id: Some(key.id.clone()),
                }));
            }
        }
        if self.vault_resource_id.is_none() || self.vault_uri.is_none() {
            return Err(missing(&key.id, "vault id or URI from Azure"));
        }
        Ok(HandlerAction::Continue {
            state: GrantingInstaller,
            suggested_delay: None,
        })
    }

    /// Lets the setup identity create the key: a vault with RBAC authorization gives its
    /// creator no data-plane rights.
    #[handler(state = GrantingInstaller, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn granting_installer(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let azure = ctx.get_azure_config()?;
        let vault_id = self
            .vault_resource_id
            .clone()
            .ok_or_else(|| missing(&key.id, "vault id"))?;
        let caller = ctx
            .service_provider
            .get_azure_key_vault_keys_client(azure)?
            .caller_identity()
            .await
            .context(ErrorData::CloudPlatformError {
                message: "Failed to work out which identity runs setup".to_string(),
                resource_id: Some(key.id.clone()),
            })?;
        let assignment_name = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!(
                "deployment:azure:key-installer:{}:{}",
                ctx.resource_prefix, key.id
            )
            .as_bytes(),
        );
        let full_id = format!(
            "{vault_id}/providers/Microsoft.Authorization/roleAssignments/{assignment_name}"
        );
        let assignment = RoleAssignment {
            id: None,
            name: None,
            type_: None,
            properties: Some(RoleAssignmentProperties {
                principal_id: caller.object_id,
                role_definition_id: format!(
                    "/subscriptions/{}/providers/Microsoft.Authorization/roleDefinitions/{CRYPTO_OFFICER_ROLE}",
                    azure.subscription_id
                ),
                scope: Some(vault_id),
                principal_type: if caller.is_app {
                    RoleAssignmentPropertiesPrincipalType::ServicePrincipal
                } else {
                    RoleAssignmentPropertiesPrincipalType::User
                },
                condition: None,
                condition_version: None,
                delegated_managed_identity_resource_id: None,
                description: Some("Lets setup create the encryption key".to_string()),
                created_by: None,
                created_on: None,
                updated_by: None,
                updated_on: None,
            }),
        };
        match ctx
            .service_provider
            .get_azure_authorization_client(azure)?
            .create_or_update_role_assignment_by_id(full_id.clone(), &assignment)
            .await
        {
            Ok(_) => {}
            // An earlier attempt made it: the id is deterministic.
            Err(error)
                if matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceConflict { .. })
                ) => {}
            Err(error) => {
                return Err(error.context(ErrorData::CloudPlatformError {
                    message: "Failed to grant the setup identity access to create the key"
                        .to_string(),
                    resource_id: Some(key.id.clone()),
                }));
            }
        }
        self.installer_assignment_id = Some(full_id);
        Ok(HandlerAction::Continue {
            state: CreatingKey,
            suggested_delay: None,
        })
    }

    #[handler(state = CreatingKey, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn creating_key(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let azure = ctx.get_azure_config()?;
        let vault_uri = self
            .vault_uri
            .clone()
            .ok_or_else(|| missing(&key.id, "vault URI"))?;
        let client = ctx.service_provider.get_azure_key_vault_keys_client(azure)?;
        let tags = resource_tags(ctx.resource_prefix, &key.id)
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let versionless = format!("{}/keys/{KEY_NAME}", vault_uri.trim_end_matches('/'));

        // A key under our name is an earlier attempt of this run, or someone else's: it is
        // used only when its metadata is what Alien creates.
        let bundle = match client.get_key(&versionless).await {
            Ok(existing) => {
                check_key_adoptable(&existing, &tags, &versionless, &key.id)?;
                existing
            }
            Err(error) => {
                let not_found = matches!(
                    error.error,
                    Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                );
                // 403 right after the role assignment is RBAC propagation: the key does not
                // exist yet, or we could not tell. Wait and try again.
                let forbidden = super::cloud_response(&error)
                    .is_some_and(|(status, _, _)| status == 403);
                if !not_found && !forbidden {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: "Failed to look for the encryption key".to_string(),
                        resource_id: Some(key.id.clone()),
                    }));
                }
                match client
                    .create_key(
                        &vault_uri,
                        KEY_NAME,
                        CreateKeyRequest {
                            kty: "RSA".to_string(),
                            key_size: 2048,
                            key_ops: ["decrypt", "encrypt", "unwrapKey", "wrapKey"]
                                .map(str::to_string)
                                .to_vec(),
                            attributes: CreateKeyAttributes { enabled: true },
                            tags: tags.clone(),
                        },
                    )
                    .await
                {
                    Ok(created) => created,
                    Err(error)
                        if super::cloud_response(&error)
                            .is_some_and(|(status, _, _)| status == 403)
                            && self.key_create_attempts < MAX_KEY_CREATE_ATTEMPTS =>
                    {
                        self.key_create_attempts += 1;
                        return Ok(HandlerAction::Continue {
                            state: CreatingKey,
                            suggested_delay: Some(Duration::from_secs(15)),
                        });
                    }
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: "Failed to create the encryption key. If this is a 403, the \
                                      Key Vault Crypto Officer role assignment on the vault may \
                                      not have propagated, or the setup identity cannot make it"
                                .to_string(),
                            resource_id: Some(key.id.clone()),
                        }));
                    }
                }
            }
        };
        let kid = bundle.key.kid.clone();
        let (_, version) = kid
            .rsplit_once('/')
            .ok_or_else(|| missing(&key.id, "key version from Azure"))?;
        self.key_name = Some(KEY_NAME.to_string());
        self.lineage_version_id = Some(version.to_string());
        self.key_id = Some(kid);
        self.origin = super::KeyOrigin::Created;
        Ok(HandlerAction::Continue {
            state: ApplyingRoleAssignments,
            suggested_delay: None,
        })
    }

    #[handler(state = ApplyingRoleAssignments, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn applying_role_assignments(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<HandlerAction> {
        self.apply_role_assignments(ctx).await?;
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── UPDATE FLOW ──────────────────────────────

    #[flow_entry(Update, from = [Ready, RefreshFailed])]
    #[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
    async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        if self.origin == super::KeyOrigin::Created && super::may_write_keys(ctx) {
            self.apply_role_assignments(ctx).await?;
        }
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: None,
        })
    }

    // ─────────────── DELETE FLOW ──────────────────────────────

    /// Removes this deployment's role assignments and keeps the vault, the key, the setup
    /// identity's assignment and the `<prefix>-keys` group: nothing in Alien deletes key
    /// material, and purge protection would keep the vault name reserved anyway.
    #[flow_entry(Delete)]
    #[handler(state = DeleteStart, on_failure = DeleteFailed, status = ResourceStatus::Deleting)]
    async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        let key = ctx.desired_resource_config::<Key>()?;
        if self.key_id.is_none() {
            return Ok(HandlerAction::Continue {
                state: Deleted,
                suggested_delay: None,
            });
        }
        if self.origin != super::KeyOrigin::Created {
            return Err(AlienError::new(ErrorData::ResourceConfigInvalid {
                resource_id: Some(key.id.clone()),
                message: format!(
                    "Key '{}' is owned by the setup that created it; remove it with that setup",
                    key.id
                ),
            }));
        }
        // Role assignments on the key outlive the deployment's identities as orphans unless
        // removed, and only the setup identity may edit them.
        if super::may_write_keys(ctx) && !self.role_assignment_ids.is_empty() {
            let authorization = ctx
                .service_provider
                .get_azure_authorization_client(ctx.get_azure_config()?)?;
            while let Some(id) = self.role_assignment_ids.last().cloned() {
                match authorization.delete_role_assignment_by_id(id.clone()).await {
                    Ok(_) => {}
                    Err(error)
                        if matches!(
                            error.error,
                            Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                        ) => {}
                    Err(error) => {
                        return Err(error.context(ErrorData::CloudPlatformError {
                            message: format!("Failed to remove key role assignment '{id}'"),
                            resource_id: Some(key.id.clone()),
                        }));
                    }
                }
                self.role_assignment_ids.pop();
            }
        }
        Ok(HandlerAction::Continue {
            state: Deleted,
            suggested_delay: None,
        })
    }

    #[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
    async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // A failed check keeps the last observation unless Key Vault answered about
        // the key itself; a throttle or 5xx must not pause every dependent resource.
        let config = ctx.desired_resource_config::<Key>()?;
        let versioned_key_id = self.key_id.clone().ok_or_else(|| {
            AlienError::new(ErrorData::ResourceStateSerializationFailed {
                resource_id: config.id.clone(),
                message: "Imported Azure key is missing its data-plane key ID".to_string(),
            })
        })?;
        let current_key_id = versioned_key_id
            .rsplit_once('/')
            .map(|(id, _)| id)
            .ok_or_else(|| {
                AlienError::new(ErrorData::ResourceStateSerializationFailed {
                    resource_id: config.id.clone(),
                    message: "Imported Azure key ID does not contain a version".to_string(),
                })
            })?;
        let key = ctx
            .service_provider
            .get_azure_key_vault_keys_client(ctx.get_azure_config()?)?
            .get_key(current_key_id)
            .await
            .inspect_err(|error| {
                if super::key_revoked_by_status(error, current_key_id) {
                    self.available = Some(false);
                }
            })
            .context(ErrorData::CloudPlatformError {
                message: "Failed to read Azure Key Vault key metadata".to_string(),
                resource_id: Some(config.id.clone()),
            })?;
        if !key.key.kid.starts_with(&format!("{current_key_id}/")) {
            return Err(AlienError::new(ErrorData::ResourceDrift {
                resource_id: config.id.clone(),
                message: "Azure Key Vault returned metadata for a different key".to_string(),
            }));
        }
        self.key_id = Some(key.key.kid.clone());
        self.available = Some(key.attributes.enabled == Some(true));
        let (health, lifecycle) = match key.attributes.enabled {
            Some(true) => (ObservedHealth::Healthy, ProviderLifecycleState::Running),
            Some(false) => (ObservedHealth::Unhealthy, ProviderLifecycleState::Stopped),
            None => (ObservedHealth::Degraded, ProviderLifecycleState::Unknown),
        };
        ctx.emit_heartbeat(ResourceHeartbeat {
            deployment_id: None,
            resource_id: config.id.clone(),
            resource_type: Key::RESOURCE_TYPE,
            controller_platform: Platform::Azure,
            backend: HeartbeatBackend::Azure,
            observed_at: Utc::now(),
            data: ResourceHeartbeatData::Key(KeyHeartbeatData::AzureKeyVault(
                AzureKeyVaultKeyHeartbeatData {
                    status: KeyHeartbeatStatus {
                        health,
                        lifecycle,
                        message: (self.available != Some(true))
                            .then(|| "customer key unavailable".to_string()),
                    },
                    key_id: key.key.kid,
                    enabled: key.attributes.enabled,
                    key_type: key.key.kty,
                    key_operations: key.key.key_ops,
                    recovery_level: key.attributes.recovery_level,
                },
            )),
            raw: vec![],
        });
        Ok(HandlerAction::Continue {
            state: Ready,
            suggested_delay: Some(Duration::from_secs(30)),
        })
    }

    terminal_state!(
        state = CreateFailed,
        status = ResourceStatus::ProvisionFailed
    );
    terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
    terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
    terminal_state!(
        state = RefreshFailed,
        status = ResourceStatus::RefreshFailed
    );
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);

    fn requires_convergence_reconciliation(&self) -> bool {
        true
    }

    fn needs_update(&self, ctx: &ResourceControllerContext<'_>) -> Result<bool> {
        if self.origin != super::KeyOrigin::Created || !super::may_write_keys(ctx) {
            return Ok(false);
        }
        let key = ctx.desired_resource_config::<Key>()?;
        Ok(self.access_revision.as_deref() != Some(desired_revision(ctx, &key.id)?.as_str()))
    }

    fn build_outputs(&self) -> Option<ResourceOutputs> {
        Some(ResourceOutputs::new(KeyOutputs {
            // A key whose checks kept failing until RefreshFailed is no longer
            // observed, so its last value would never change again.
            available: Some(self.state == AzureKeyState::Ready && self.available == Some(true)),
            fingerprint: KeyFingerprint::Azure {
                vault_resource_id: self.vault_resource_id.clone()?,
                key_name: self.key_name.clone()?,
                lineage_version_id: self.lineage_version_id.clone()?,
            },
            wrapping_key_id: self.key_id.clone()?,
        }))
    }

    fn get_binding_params(&self) -> Result<Option<serde_json::Value>> {
        let Some(key_id) = &self.key_id else {
            return Ok(None);
        };
        Ok(Some(
            serde_json::to_value(KeyBinding::azure_key_vault(key_id))
                .into_alien_error()
                .context(ErrorData::ResourceStateSerializationFailed {
                    resource_id: "binding".to_string(),
                    message: "Failed to serialize Key binding parameters".to_string(),
                })?,
        ))
    }
}


impl AzureKeyController {
    fn group_and_vault(&self, key_id: &str) -> Result<(String, String)> {
        match (&self.resource_group_name, &self.vault_name) {
            (Some(group), Some(vault)) => Ok((group.clone(), vault.clone())),
            _ => Err(missing(key_id, "vault name or resource group")),
        }
    }

    /// Assigns the plan's roles at the key's scope (reader for management, encrypt/decrypt
    /// for the access and runtime identities) and removes assignments an earlier run made
    /// that the plan no longer has.
    async fn apply_role_assignments(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<()> {
        let key = ctx.desired_resource_config::<Key>()?;
        if !super::may_write_keys(ctx) {
            return Err(super::key_created_by_setup(&key.id));
        }
        let (group, vault) = self.group_and_vault(&key.id)?;
        let scope_group = group.clone();
        let scope = Scope::Resource {
            resource_group_name: group,
            resource_provider: "Microsoft.KeyVault".to_string(),
            parent_resource_path: Some(format!("vaults/{vault}")),
            resource_type: "keys".to_string(),
            resource_name: KEY_NAME.to_string(),
        };
        // The key permission sets use `${resourceName}` as the whole assignment scope, so it
        // is the key's full resource id, not its short name.
        let key_resource_id = format!(
            "/subscriptions/{}/resourceGroups/{}/providers/Microsoft.KeyVault/vaults/{}/keys/{KEY_NAME}",
            ctx.get_azure_config()?.subscription_id,
            scope_group,
            vault
        );
        let permission_context =
            ResourcePermissionsHelper::build_azure_permission_context(ctx, &key_resource_id)?;
        let planned = AzurePermissionsHelper::plan_resource_scoped_role_assignment_ids(
            ctx,
            &key.id,
            "key",
            scope.clone(),
            &permission_context,
        )
        .await?;
        ResourcePermissionsHelper::apply_azure_resource_scoped_permissions(
            ctx, &key.id, &key_resource_id, scope, "Key", "key",
        )
        .await?;
        let authorization = ctx
            .service_provider
            .get_azure_authorization_client(ctx.get_azure_config()?)?;
        for stale in self.role_assignment_ids.iter().filter(|id| !planned.contains(id)) {
            match authorization.delete_role_assignment_by_id(stale.clone()).await {
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.error,
                        Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
                    ) => {}
                Err(error) => {
                    return Err(error.context(ErrorData::CloudPlatformError {
                        message: format!("Failed to remove stale key role assignment '{stale}'"),
                        resource_id: Some(key.id.clone()),
                    }));
                }
            }
        }
        self.role_assignment_ids = planned;
        self.access_revision = Some(desired_revision(ctx, &key.id)?);
        Ok(())
    }
}

fn missing(key_id: &str, what: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceStateSerializationFailed {
        resource_id: key_id.to_string(),
        message: format!("Azure key state is missing its {what}"),
    })
}

/// `<prefix>-keys`: holds the vaults and is never deleted by Alien.
pub(crate) fn keys_group_name(prefix: &str) -> String {
    format!("{prefix}-keys")
}

/// `<prefix>-<id>` as the Terraform setup names it (17 characters), plus a digest of the full
/// name: vault names are global, 24 characters at most, and stay reserved after deletion. The
/// digest replaces Terraform's random suffix so a retry computes the same vault.
pub(crate) fn vault_name(prefix: &str, key_id: &str) -> String {
    let base: String = format!("{prefix}-{key_id}")
        .to_lowercase()
        .replace('_', "-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(17)
        .collect();
    let digest = Sha256::digest(format!("{prefix}/{key_id}").as_bytes());
    let suffix: String = digest.iter().take(3).map(|b| format!("{b:02x}")).collect();
    let base = base.trim_matches('-');
    let base = if base.starts_with(|c: char| c.is_ascii_digit()) {
        format!("k{base}")
    } else {
        base.to_string()
    };
    format!("{base}-{suffix}")
}

fn resource_tags(prefix: &str, key_id: &str) -> HashMap<String, String> {
    alien_core::setup_resource_tags(prefix, key_id, "key")
        .into_iter()
        .collect()
}

fn check_vault_adoptable(
    existing: &alien_azure_clients::models::keyvault::Vault,
    tenant_id: &Uuid,
    tags: &HashMap<String, String>,
    name: &str,
    key_id: &str,
) -> Result<()> {
    let properties = &existing.properties;
    let refuse = |why: &str| {
        Err(AlienError::new(ErrorData::ResourceDrift {
            resource_id: key_id.to_string(),
            message: format!(
                "Key Vault '{name}' already exists and is not a vault Alien creates ({why}); refusing to adopt it"
            ),
        }))
    };
    if &properties.tenant_id != tenant_id {
        return refuse("it belongs to another tenant");
    }
    if !properties.enable_rbac_authorization || !properties.access_policies.is_empty() {
        return refuse("it does not use role-based access control only");
    }
    if properties.enable_purge_protection != Some(true) || !properties.enable_soft_delete {
        return refuse("it has no purge protection");
    }
    if tags
        .iter()
        .any(|(tag, value)| existing.tags.get(tag) != Some(value))
    {
        return refuse("it does not carry this deployment's tags");
    }
    Ok(())
}

fn check_key_adoptable(
    existing: &alien_azure_clients::keyvault::KeyBundle,
    tags: &BTreeMap<String, String>,
    url: &str,
    key_id: &str,
) -> Result<()> {
    let refuse = |why: &str| {
        Err(AlienError::new(ErrorData::ResourceDrift {
            resource_id: key_id.to_string(),
            message: format!(
                "Key '{url}' already exists and is not a key Alien creates ({why}); refusing to adopt it"
            ),
        }))
    };
    if !existing.key.kid.starts_with(&format!("{url}/")) {
        return refuse("it is a different key");
    }
    if existing.key.kty != "RSA" {
        return refuse(&format!("key type {}", existing.key.kty));
    }
    let bits = existing
        .key
        .n
        .as_ref()
        .map(|n| n.trim_end_matches('=').len() * 6 / 8 * 8);
    if !matches!(bits, Some(2048)) {
        return refuse("it is not a 2048-bit key");
    }
    let mut ops = existing.key.key_ops.clone();
    ops.sort();
    if ops != ["decrypt", "encrypt", "unwrapKey", "wrapKey"] {
        return refuse("it allows other operations");
    }
    if existing.attributes.enabled != Some(true) {
        return refuse("it is disabled");
    }
    if tags.iter().any(|(tag, value)| existing.tags.get(tag) != Some(value)) {
        return refuse("it does not carry this deployment's tags");
    }
    Ok(())
}

/// Changes when the grants a created key depends on change.
fn desired_revision(ctx: &ResourceControllerContext<'_>, key_id: &str) -> Result<String> {
    let plan = alien_permissions::key_access::key_access_plan(ctx.desired_stack, key_id).context(
        ErrorData::ResourceConfigInvalid {
            resource_id: Some(key_id.to_string()),
            message: "Failed to work out which identities may use the key".to_string(),
        },
    )?;
    Ok(super::access_revision(&[&format!("{plan:?}")]))
}

#[derive(Debug, Default)]
pub struct AzureKeyImporter;

impl ResourceImporter for AzureKeyImporter {
    type ImportData = AzureKeyImportData;

    fn merge_reimport(
        &self,
        existing: StackResourceState,
        imported: StackResourceState,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        super::merge_key_reimport(existing, imported, ctx)
    }

    fn import(
        &self,
        data: Self::ImportData,
        ctx: &ImportContext<'_>,
    ) -> alien_core::Result<StackResourceState> {
        make_imported_state(
            AzureKeyController {
                state: AzureKeyState::Ready,
                available: None,
                vault_resource_id: Some(data.vault_resource_id),
                key_name: Some(data.key_name),
                lineage_version_id: Some(data.lineage_version_id),
                key_id: Some(data.key_id),
                origin: super::KeyOrigin::Imported,
                vault_name: None,
                resource_group_name: None,
                vault_uri: None,
                installer_assignment_id: None,
                key_create_attempts: 0,
                access_revision: None,
                role_assignment_ids: Vec::new(),
                _internal_stay_count: None,
            },
            ctx,
        )
    }
}

#[cfg(all(test, feature = "test"))]
#[path = "azure_tests.rs"]
mod azure_tests;
