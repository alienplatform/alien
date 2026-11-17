//! Azure Postgres — Database for PostgreSQL Flexible Server, private-only via a Private
//! Endpoint and a `privatelink.postgres.database.azure.com` private DNS zone.
//!
//! Mirrors the live-resource create flow in
//! runtime controller so the setup package (push deployment)
//! and the controller (pull deployment) converge on the same cloud resources:
//!
//!   1. A generated master password stored as an `azurerm_key_vault_secret` in the stack's
//!      Key Vault. Ephemeral reads and write-only arguments keep values out of state.
//!   2. The Flexible Server with `public_network_access_enabled = false` — never publicly
//!      reachable.
//!   3. A dedicated Private Endpoint subnet, the Private Endpoint targeting the server's
//!      `postgresqlServer` sub-resource, a private DNS zone + VNet link + zone group so the
//!      server FQDN resolves to the endpoint's private IP inside the VNet.
//!   4. The default `alien` database.
//!   5. Explicit `postgres/data-access` grants read only this database password secret.
//!      `postgres/management` grants control-plane access scoped to this server.

use crate::emitters::azure::cmek::grant_store_key;
use crate::emitters::postgres_credentials::{
    database_revision, generated_password, password_revision,
};
use alien_core::{
    import::EmitContext, ErrorData, PermissionProfile, PermissionSetReference, Postgres,
    RemoteStackManagement, Result, Vault,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use crate::block::ephemeral_block;
use crate::block::{attr, block, data_block, nested, resource_block};
use crate::emitters::azure::helpers::{
    downcast, permission_context, required_label, service_account_principal_id,
    setup_execution_role_label, setup_management_role_label,
};
use crate::expr;
use crate::{TfEmitter, TfFragment};
use hcl::expr::Expression;

/// Port Flexible Server listens on; fixed.
const POSTGRES_PORT: i64 = 5432;
/// Administrator login; fixed across backends and what the binding/import advertise. Azure
/// forbids `admin`/`root`/`azure_superuser`, and `alien` is a valid identifier.
const MASTER_USERNAME: &str = alien_core::POSTGRES_ADMIN_USERNAME;
/// Default database name created on the server, from the single source of truth
/// `alien_core::POSTGRES_DATABASE_NAME` — shared with the controller, importer, and the other
/// emitters so a frozen import always binds to a database that exists.
const DATABASE_NAME: &str = alien_core::POSTGRES_DATABASE_NAME;
/// The Private DNS zone Flexible Server private endpoints resolve through; fixed by Azure.
const PRIVATE_DNS_ZONE: &str = "privatelink.postgres.database.azure.com";
/// The `groupIds` sub-resource selector that wires a Private Endpoint to a Flexible Server.
const PRIVATE_LINK_GROUP_ID: &str = "postgresqlServer";

/// The Flexible Server SKUs, with the `B_`/`GP_`-prefixed names the azurerm provider's `sku_name`
/// argument wants, as `(cpu_cap_vcpu, mem_cap_gib, sku_name)` ascending. MUST stay in lockstep with
/// the runtime controller's `AZURE_POSTGRES_SKUS` (same caps + ordering;
/// the controller carries the unprefixed names its ARM client wants): the push (this emitter) and
/// pull (controller) paths must pick the same SKU for the same config, so the cross-crate parity
/// test pins them equal.
const AZURE_POSTGRES_SKUS: &[(f64, f64, &str)] = &[
    (1.0, 2.0, "B_Standard_B1ms"),
    (2.0, 4.0, "B_Standard_B2s"),
    (2.0, 8.0, "GP_Standard_D2ds_v5"),
    (4.0, 16.0, "GP_Standard_D4ds_v5"),
    (8.0, 32.0, "GP_Standard_D8ds_v5"),
];

#[derive(Debug, Clone, Copy, Default)]
pub struct AzurePostgresEmitter;

impl TfEmitter for AzurePostgresEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let postgres = downcast::<Postgres>(ctx, Postgres::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let network_label = network_label(ctx)?.to_string();
        // The Private Endpoint reaches the VNet differently per network mode: in a create-mode VNet
        // (Alien-owned) we carve a dedicated PE subnet; in a bring-your-own VNet we look up the
        // customer's dedicated PE subnet by name and reference the VNet as a data source — mirroring
        // how the Azure network emitter materializes a BYO VNet. Resolved once, used below.
        let pg_network = azure_postgres_network(ctx)?;
        let byo = matches!(pg_network, AzurePostgresNetwork::Byo { .. });
        let vault_label = vault_label(ctx)?.to_string();
        let pe_subnet_label = format!("{label}_pe_subnet");
        let pe_label = format!("{label}_pe");
        let dns_zone_label = format!("{label}_dns_zone");
        let dns_link_label = format!("{label}_dns_link");

        let mut fragment = TfFragment::default();

        // 1. Generated master password written as a Key Vault secret. The random_password feeds
        //    the server's administrator login and the secret; its raw value never enters
        //    import/binding refs — only the secret URI does.
        let revision = password_revision(&mut fragment, label);
        fragment.ephemeral_blocks.push(ephemeral_block(
            "azurerm_key_vault_secret",
            label,
            [
                attr(
                    "key_vault_id",
                    expr::traversal(["azurerm_key_vault", &vault_label, "id"]),
                ),
                attr(
                    "name",
                    expr::traversal(["azurerm_key_vault_secret", label, "name"]),
                ),
                attr(
                    "version",
                    expr::traversal(["azurerm_key_vault_secret", label, "version"]),
                ),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "azurerm_key_vault_secret",
            label,
            [
                attr(
                    "name",
                    expr::template(format!(
                        "${{local.resource_prefix}}-{}-postgres-password",
                        postgres.id
                    )),
                ),
                attr("value_wo", generated_password(label)),
                attr("value_wo_version", revision),
                attr(
                    "key_vault_id",
                    expr::traversal(["azurerm_key_vault", &vault_label, "id"]),
                ),
            ],
        ));

        // 2. The Flexible Server — public network access disabled so it is reachable only
        //    through the Private Endpoint below.
        let mut server_body = vec![
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{}", postgres.id)),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr("version", Expression::String(postgres.version.clone())),
            attr(
                "administrator_login",
                Expression::String(MASTER_USERNAME.to_string()),
            ),
            attr(
                "administrator_password_wo",
                expr::traversal(["ephemeral", "azurerm_key_vault_secret", label, "value"]),
            ),
            attr(
                "administrator_password_wo_version",
                database_revision(&format!("azurerm_key_vault_secret.{label}.version")),
            ),
            attr(
                "sku_name",
                Expression::String(azure_postgres_sku_name(postgres)?.to_string()),
            ),
            attr(
                "storage_mb",
                Expression::Number(hcl::Number::from(storage_mb(postgres)?)),
            ),
            attr(
                "backup_retention_days",
                Expression::Number(hcl::Number::from(7i64)),
            ),
            attr("public_network_access_enabled", Expression::Bool(false)),
            // No `zone`: Azure picks a zone the subscription can use (or none where it can use
            // none). A pinned zone "1" fails in regions where the subscription has no access to it.
            // Servers installed with zone "1" keep it: without `ignore_changes`, a plan against
            // their state shows `zone = "1" -> null` as an in-place change.
            nested(block(
                "lifecycle",
                [attr("ignore_changes", expr::parse("[zone]")?)],
            )),
        ];
        // Zone-redundant HA when requested, mirroring the controller (config.high_availability ->
        // high_availability { mode = "ZoneRedundant" }). Azure picks the primary zone and
        // the standby zone.
        if postgres.high_availability {
            server_body.push(nested(block(
                "high_availability",
                [attr(
                    "mode",
                    Expression::String("ZoneRedundant".to_string()),
                )],
            )));
        }
        // A customer key encrypts the server's data and backups. Flexible Server accepts it only
        // at create, through a user-assigned identity that may use the key.
        if let Some(key) = postgres.encryption_key.as_ref() {
            let store_key = grant_store_key(ctx, &mut fragment, key, label, "postgres-encryption")?;
            server_body.push(nested(store_key.identity_block));
            server_body.push(nested(block(
                "customer_managed_key",
                [
                    attr("key_vault_key_id", store_key.key_id),
                    attr("primary_user_assigned_identity_id", store_key.identity_id),
                ],
            )));
            server_body.push(attr(
                "depends_on",
                expr::raw(format!("[{}]", store_key.role_dependency)),
            ));
        }
        fragment.resource_blocks.push(resource_block(
            "azurerm_postgresql_flexible_server",
            label,
            server_body,
        ));

        // `azure.extensions` gates `CREATE EXTENSION` on Flexible Server. Pre-allow Alien's default
        // supported set — pgvector (the embeddings/RAG use case) plus the tiny, ubiquitous contrib
        // extensions (pg_trgm, uuid-ossp, pgcrypto, btree_gin) available on every backend — so a workload's
        // migrations can `CREATE EXTENSION` them. Mirrors the controller's `ConfiguringExtensions`
        // step; only Azure needs the explicit allowlist (AWS/GCP ship these pre-available).
        fragment.resource_blocks.push(resource_block(
            "azurerm_postgresql_flexible_server_configuration",
            &format!("{label}_extensions"),
            [
                attr("name", Expression::String("azure.extensions".to_string())),
                attr(
                    "server_id",
                    expr::traversal(["azurerm_postgresql_flexible_server", label, "id"]),
                ),
                attr(
                    "value",
                    Expression::String("VECTOR,PG_TRGM,UUID-OSSP,PGCRYPTO,BTREE_GIN".to_string()),
                ),
            ],
        ));

        // 3. Reachability: the network's dedicated Private Endpoint subnet (created by the network
        //    emitter, or the customer's in a BYO VNet), the Private Endpoint targeting the
        //    server's `postgresqlServer` sub-resource, and a private DNS zone + VNet link + zone
        //    group so the server FQDN resolves to the endpoint's private IP. The PE needs its OWN
        //    subnet — the stack's private subnet is the Container Apps infrastructure subnet and
        //    cannot be shared.
        match &pg_network {
            AzurePostgresNetwork::Create => {
                // The Azure network emitter already creates the dedicated `-pe` subnet in a
                // create-mode VNet; reuse it. A second subnet here would claim the same address
                // range and Azure rejects the overlap.
            }
            AzurePostgresNetwork::Byo { pe_subnet_name } => {
                // BYO VNet: never carve into a VNet Alien does not own. Look up the customer's
                // dedicated PE subnet (the required `private_endpoint_subnet_name`) as a data source.
                fragment.data_blocks.push(data_block(
                    "azurerm_subnet",
                    &pe_subnet_label,
                    [
                        attr("name", Expression::String(pe_subnet_name.clone())),
                        attr(
                            "virtual_network_name",
                            expr::traversal([
                                "data",
                                "azurerm_virtual_network",
                                &network_label,
                                "name",
                            ]),
                        ),
                        attr(
                            "resource_group_name",
                            expr::traversal([
                                "data",
                                "azurerm_virtual_network",
                                &network_label,
                                "resource_group_name",
                            ]),
                        ),
                    ],
                ));
            }
        }

        fragment.resource_blocks.push(resource_block(
            "azurerm_private_dns_zone",
            &dns_zone_label,
            [
                attr("name", Expression::String(PRIVATE_DNS_ZONE.to_string())),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "azurerm_private_dns_zone_virtual_network_link",
            &dns_link_label,
            [
                attr(
                    "name",
                    expr::template(format!(
                        "${{local.resource_prefix}}-{}-dns-link",
                        postgres.id
                    )),
                ),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr(
                    "private_dns_zone_name",
                    expr::traversal(["azurerm_private_dns_zone", &dns_zone_label, "name"]),
                ),
                attr(
                    "virtual_network_id",
                    vnet_attr_traversal(byo, &network_label, "id"),
                ),
                // The zone group registers the endpoint's A record; auto-registration must stay
                // off so the two don't fight over the name (matches the controller).
                attr("registration_enabled", Expression::Bool(false)),
            ],
        ));

        fragment.resource_blocks.push(resource_block(
            "azurerm_private_endpoint",
            &pe_label,
            [
                attr(
                    "name",
                    expr::template(format!("${{local.resource_prefix}}-{}-pe", postgres.id)),
                ),
                attr(
                    "resource_group_name",
                    expr::raw("var.azure_resource_group_name"),
                ),
                attr("location", expr::raw("var.azure_location")),
                attr(
                    "subnet_id",
                    pe_subnet_id_traversal(byo, &network_label, &pe_subnet_label),
                ),
                nested(block(
                    "private_service_connection",
                    [
                        attr(
                            "name",
                            expr::template(format!(
                                "${{local.resource_prefix}}-{}-psc",
                                postgres.id
                            )),
                        ),
                        attr("is_manual_connection", Expression::Bool(false)),
                        attr(
                            "private_connection_resource_id",
                            expr::traversal(["azurerm_postgresql_flexible_server", label, "id"]),
                        ),
                        attr(
                            "subresource_names",
                            Expression::Array(vec![Expression::String(
                                PRIVATE_LINK_GROUP_ID.to_string(),
                            )]),
                        ),
                    ],
                )),
                nested(block(
                    "private_dns_zone_group",
                    [
                        attr("name", Expression::String("default".to_string())),
                        attr(
                            "private_dns_zone_ids",
                            Expression::Array(vec![expr::traversal([
                                "azurerm_private_dns_zone",
                                &dns_zone_label,
                                "id",
                            ])]),
                        ),
                    ],
                )),
            ],
        ));

        // 4. The default `alien` database. `depends_on` the extensions configuration: both are
        //    server-level operations that only reference the server, so Terraform would otherwise
        //    apply them concurrently and Azure rejects one with `ServerIsBusy` ("cannot complete
        //    operation while server is busy processing another operation"). Serialising them also
        //    matches the controller order (`SettingExtensions` runs before `CreatingDatabase`).
        fragment.resource_blocks.push(resource_block(
            "azurerm_postgresql_flexible_server_database",
            label,
            [
                attr("name", Expression::String(DATABASE_NAME.to_string())),
                attr(
                    "server_id",
                    expr::traversal(["azurerm_postgresql_flexible_server", label, "id"]),
                ),
                attr(
                    "depends_on",
                    expr::raw(format!(
                        "[azurerm_postgresql_flexible_server_configuration.{label}_extensions]"
                    )),
                ),
            ],
        ));

        // 5. postgres/data-access + postgres/management role assignments.
        emit_postgres_permissions(ctx, label, &mut fragment)?;

        fragment.retain_resources(|kind, name| {
            matches!(
                kind,
                "azurerm_postgresql_flexible_server"
                    | "azurerm_postgresql_flexible_server_database"
                    | "azurerm_postgresql_flexible_server_configuration"
            ) || (kind == "azurerm_user_assigned_identity" && name == format!("{label}_encryption"))
                || (kind == "azurerm_role_assignment" && name == format!("{label}_encryption_key"))
        });
        // Secret versions must still rotate; preserve recovery state without blocking replacement.
        fragment.retain_state(|kind, _| matches!(kind, "azurerm_key_vault_secret"));
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        let pe_label = format!("{label}_pe");
        // Exactly the `AzureFlexibleServerPostgresImportData` field set.
        Ok(expr::object([
            (
                "serverName",
                expr::traversal(["azurerm_postgresql_flexible_server", label, "name"]),
            ),
            (
                "serverResourceId",
                expr::traversal(["azurerm_postgresql_flexible_server", label, "id"]),
            ),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "database",
                expr::traversal(["azurerm_postgresql_flexible_server_database", label, "name"]),
            ),
            // Host is the server's own FQDN; the private DNS zone shadows it to the endpoint's
            // private IP inside the VNet.
            (
                "host",
                expr::traversal(["azurerm_postgresql_flexible_server", label, "fqdn"]),
            ),
            (
                "keyVaultUrl",
                expr::traversal([
                    "azurerm_key_vault",
                    &vault_label(ctx)?.to_string(),
                    "vault_uri",
                ]),
            ),
            (
                "passwordSecretName",
                expr::traversal(["azurerm_key_vault_secret", label, "name"]),
            ),
            (
                "passwordSecretUri",
                expr::traversal(["azurerm_key_vault_secret", label, "id"]),
            ),
            (
                "privateEndpointName",
                expr::traversal(["azurerm_private_endpoint", &pe_label, "name"]),
            ),
            ("region", expr::raw("var.azure_location")),
            (
                "version",
                expr::traversal(["azurerm_postgresql_flexible_server", label, "version"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("flexible-server".to_string())),
            // Host is the server FQDN, resolvable only inside the VNet through the private DNS
            // zone — the only path workloads have to the private-only server.
            (
                "host",
                expr::traversal(["azurerm_postgresql_flexible_server", label, "fqdn"]),
            ),
            ("port", Expression::Number(hcl::Number::from(POSTGRES_PORT))),
            ("database", Expression::String(DATABASE_NAME.to_string())),
            ("username", Expression::String(MASTER_USERNAME.to_string())),
            // The binding carries the Key Vault secret URI; the workload resolves the value at
            // load time. The raw password never appears here.
            (
                "passwordSecretUri",
                expr::traversal(["azurerm_key_vault_secret", label, "id"]),
            ),
        ])))
    }
}

/// The Flexible Server SKU for `postgres`, honoring requested cpu AND memory: the smallest SKU
/// whose vCPU and memory caps both satisfy the request. Unset cpu/memory -> the smallest burstable
/// SKU without HA; HA requires General Purpose and adds standby cost; so does a customer key (Flexible Server refuses a customer-managed key on Burstable with `SkuNotAvailable`, proven on a real subscription). A malformed value fails fast (matching the controller) rather than silently
/// under-provisioning the push path; a request beyond the largest SKU errors. Mirrors
/// `AzurePostgresController::resolve_sku` (which carries the unprefixed names its ARM client wants);
/// `pub` so a parity test against the runtime controller can call it.
pub fn azure_postgres_sku_name(postgres: &Postgres) -> Result<&'static str> {
    let req_cpu = requested_cpu(postgres)?;
    let req_mem = requested_memory_gib(postgres)?;
    AZURE_POSTGRES_SKUS
        .iter()
        .find(|(cpu_cap, mem_cap, name)| {
            *cpu_cap >= req_cpu
                && *mem_cap >= req_mem
                && (!(postgres.high_availability || postgres.encryption_key.is_some())
                    || name.starts_with("GP_"))
                && (postgres.high_availability
                    || postgres.encryption_key.is_some()
                    || *name != "GP_Standard_D2ds_v5")
        })
        .map(|(_, _, name)| *name)
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "Postgres cpu/memory sizing".to_string(),
                reason:
                    "request exceeds the largest Azure Flexible Server SKU (max 8 vCPU / 32 GiB)"
                        .to_string(),
            })
        })
}

fn requested_cpu(postgres: &Postgres) -> Result<f64> {
    Ok(postgres.requested_cpu()?.unwrap_or(0.0))
}

fn requested_memory_gib(postgres: &Postgres) -> Result<f64> {
    Ok(postgres.requested_memory_gib()?.unwrap_or(0.0))
}

/// Storage sizes (GiB) Flexible Server accepts on Premium SSD. Any other size is rejected at
/// create, so a request is rounded up to the next tier (the server is grow-only).
const FLEXIBLE_SERVER_STORAGE_TIERS_GIB: [i64; 11] =
    [32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32767];

/// The Flexible Server storage size in GiB for `postgres`: the requested size (e.g. "50Gi")
/// rounded up to a size Flexible Server accepts (32, 64, 128 ... GiB), floored at 32. A malformed
/// value, or one above the 32 TiB maximum, fails fast rather than being coerced. Mirrors
/// `AzurePostgresController::resolve_storage_gb`; `pub` so a parity test against the
/// runtime controller can call it.
pub fn azure_postgres_storage_gb(postgres: &Postgres) -> Result<i64> {
    let digits: String = postgres
        .storage
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let gb = digits.parse::<i64>().map_err(|_| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: "Postgres storage sizing".to_string(),
            reason: format!("invalid storage value '{}'", postgres.storage),
        })
    })?;
    let largest = *FLEXIBLE_SERVER_STORAGE_TIERS_GIB.last().unwrap();
    FLEXIBLE_SERVER_STORAGE_TIERS_GIB
        .iter()
        .copied()
        .find(|tier| *tier >= gb)
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "Postgres storage sizing".to_string(),
                reason: format!(
                    "requested storage {gb} GiB is above the Flexible Server maximum of {largest} GiB (32 TiB)"
                ),
            })
        })
}

fn storage_mb(postgres: &Postgres) -> Result<i64> {
    Ok(azure_postgres_storage_gb(postgres)? * 1024)
}

/// How the stack's network is provided, from the Postgres Private Endpoint's point of view.
enum AzurePostgresNetwork {
    /// Alien-managed VNet (`Create`/`UseDefault`): carve a dedicated Private-Endpoint subnet.
    Create,
    /// Bring-your-own VNet: the customer supplies a dedicated PE subnet by name; look it up rather
    /// than carve into a VNet Alien does not own.
    Byo { pe_subnet_name: String },
}

/// Classify the stack's network for the Postgres Private Endpoint. A BYO VNet hosting Postgres MUST
/// set `private_endpoint_subnet_name` — the Flexible Server PE needs a dedicated subnet and Alien
/// will not carve one from a VNet it does not own — so this fails fast when it is missing.
fn azure_postgres_network(ctx: &EmitContext<'_>) -> Result<AzurePostgresNetwork> {
    let settings = ctx
        .stack
        .resources()
        .find(|(_id, entry)| entry.config.resource_type() == alien_core::Network::RESOURCE_TYPE)
        .and_then(|(_id, entry)| entry.config.downcast_ref::<alien_core::Network>())
        .map(|network| &network.settings)
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit Azure Postgres setup".to_string(),
                reason: "requires a network resource, but none was found in the stack".to_string(),
            })
        })?;
    // Explicit arms (no wildcard) so a future NetworkSettings variant fails to compile here rather
    // than being silently absorbed into managed-create, mirroring require_aurora/require_cloud_sql.
    match settings {
        // Alien-managed VNet: carve a dedicated Private-Endpoint subnet.
        alien_core::NetworkSettings::Create { .. } | alien_core::NetworkSettings::UseDefault => {
            Ok(AzurePostgresNetwork::Create)
        }
        alien_core::NetworkSettings::ByoVnetAzure {
            private_endpoint_subnet_name,
            ..
        } => {
            let pe_subnet_name = private_endpoint_subnet_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::OperationNotSupported {
                    operation: "emit Azure Postgres setup".to_string(),
                    reason: "a bring-your-own VNet hosting Postgres must set \
                             `private_endpoint_subnet_name`: the Flexible Server Private Endpoint \
                             needs a dedicated subnet, and Alien will not carve one from a VNet it \
                             does not own"
                        .to_string(),
                })
            })?;
            Ok(AzurePostgresNetwork::Byo { pe_subnet_name })
        }
        // A non-Azure bring-your-own VPC cannot host an Azure Flexible Server Private Endpoint; the
        // Azure network emitter rejects this cross-cloud config too. Fail loud rather than emit a PE
        // subnet against a VNet that does not exist.
        alien_core::NetworkSettings::ByoVpcAws { .. }
        | alien_core::NetworkSettings::ByoVpcGcp { .. } => {
            Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit Azure Postgres setup".to_string(),
                reason: "the stack network is a non-Azure bring-your-own VPC; Azure Postgres \
                         requires an Alien-managed VNet or a bring-your-own Azure VNet"
                    .to_string(),
            }))
        }
    }
}

/// VNet attribute traversal matching how the Azure network emitter materializes the VNet: a managed
/// `azurerm_virtual_network` in create mode, a `data.azurerm_virtual_network` in BYO mode.
fn vnet_attr_traversal(byo: bool, label: &str, attr_name: &str) -> Expression {
    if byo {
        expr::traversal(["data", "azurerm_virtual_network", label, attr_name])
    } else {
        expr::traversal(["azurerm_virtual_network", label, attr_name])
    }
}

/// PE subnet id traversal: the network emitter's `<network>_pe` `azurerm_subnet` in create mode,
/// the looked-up `data.azurerm_subnet` in BYO mode.
fn pe_subnet_id_traversal(byo: bool, network_label: &str, pe_subnet_label: &str) -> Expression {
    if byo {
        expr::traversal(["data", "azurerm_subnet", pe_subnet_label, "id"])
    } else {
        expr::traversal(["azurerm_subnet", &format!("{network_label}_pe"), "id"])
    }
}

/// The Terraform label of the stack network resource, under which the Azure network emitter
/// materializes the VNet. Fail-fast if absent — the Private Endpoint cannot be placed without a VNet.
fn network_label<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
    ctx.stack
        .resources()
        .find(|(_id, entry)| entry.config.resource_type() == alien_core::Network::RESOURCE_TYPE)
        .and_then(|(id, _entry)| ctx.name_for(id))
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit Azure Postgres setup".to_string(),
                reason: "requires a network resource, but none was found in the stack".to_string(),
            })
        })
}

/// The Terraform label of the stack Key Vault resource, under which the Azure vault emitter
/// materializes `azurerm_key_vault`. Fail-fast if absent — the master password secret has
/// nowhere to live (the stack's shared `secrets` vault is preflight-injected).
fn vault_label<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
    ctx.stack
        .resources()
        .find(|(id, entry)| {
            id.as_str() == alien_core::SECRETS_VAULT_ID
                && entry.config.downcast_ref::<Vault>().is_some()
        })
        .and_then(|(id, _entry)| ctx.name_for(id))
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit Azure Postgres setup".to_string(),
                reason: "requires a vault resource to hold the master password secret, but none \
                         was found in the stack"
                    .to_string(),
            })
        })
}

/// Emit `postgres/*` role definitions + assignments for every workload service account plus
/// management, scoping each grant to this server. Mirrors the Azure vault emitter. `/provision`
/// sets are setup-time only and skipped. `postgres/data-access` has an empty Azure binding (the
/// connection secret lives in the shared deployment vault that workloads already read via
/// `vault/data-read`), so it emits nothing — keeping the secret VALUE off any grant authored
/// here. `resource_name` is the bare resource id so the management scope's
/// `${stackPrefix}-${resourceName}` resolves to the server name.
fn emit_postgres_permissions(
    ctx: &EmitContext<'_>,
    postgres_label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    for (profile_name, permission_set_refs) in postgres_permission_owners(ctx) {
        let Some(principal_id_expr) = service_account_principal_id(ctx, &profile_name) else {
            continue;
        };

        for permission_set_ref in permission_set_refs {
            let Some(permission_set) = permission_set_ref
                .resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if permission_set.id == "postgres/data-access" {
                emit_password_read_assignment(
                    ctx,
                    postgres_label,
                    &profile_name,
                    principal_id_expr.clone(),
                    fragment,
                )?;
                continue;
            }
            if !permission_set.id.starts_with("postgres/")
                || permission_set.id.ends_with("/provision")
                || permission_set
                    .platforms
                    .azure
                    .as_ref()
                    .map(|bindings| bindings.is_empty())
                    .unwrap_or(true)
            {
                continue;
            }

            let generator = AzureRuntimePermissionsGenerator::new();
            let permission_context =
                permission_context(postgres_label).with_resource_name(ctx.resource_id.to_string());
            let grant_plan = generator
                .generate_grant_plan(
                    &permission_set,
                    BindingTarget::Resource,
                    &permission_context,
                )
                .context(ErrorData::OperationNotSupported {
                    operation: "generate Azure Postgres grants".to_string(),
                    reason: format!(
                        "failed to generate grants for permission set '{}'",
                        permission_set.id
                    ),
                })?;

            for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
                let role_definition_id = match &binding.role_definition {
                    AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                        expr::template(role_definition_id.clone())
                    }
                    AzureRoleDefinitionRef::Custom { key } => {
                        let index = grant_plan
                            .custom_roles
                            .iter()
                            .position(|role| role.key == *key)
                            .unwrap_or(0);
                        let role_label =
                            setup_execution_role_label(&profile_name, &binding.role_name, index);
                        expr::traversal([
                            "azurerm_role_definition",
                            role_label.as_str(),
                            "role_definition_resource_id",
                        ])
                    }
                };
                emit_role_assignment(
                    fragment,
                    ctx.resource_id,
                    postgres_label,
                    &profile_name,
                    binding_index,
                    &binding.role_name,
                    role_definition_id,
                    principal_id_expr.clone(),
                    expr::traversal(["azurerm_postgresql_flexible_server", postgres_label, "id"]),
                );
            }
        }
    }

    let Some(management_label) = remote_stack_management_label(ctx) else {
        return Ok(());
    };

    let principal_id_expr = expr::traversal([
        "azurerm_user_assigned_identity",
        management_label,
        "principal_id",
    ]);

    for permission_set_ref in management_permission_refs(ctx) {
        let Some(permission_set) =
            permission_set_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
        else {
            continue;
        };
        if permission_set.id == "postgres/data-access" {
            emit_password_read_assignment(
                ctx,
                postgres_label,
                "management",
                principal_id_expr.clone(),
                fragment,
            )?;
            continue;
        }
        // Skip non-postgres sets (a resource-specific profile may carry other sets), `/provision`
        // (setup emits management/access, not ownership), and azure-none — matching the AWS/GCP
        // Postgres emitters.
        if !permission_set.id.starts_with("postgres/")
            || permission_set.id.ends_with("/provision")
            || permission_set.platforms.azure.is_none()
        {
            continue;
        }

        let generator = AzureRuntimePermissionsGenerator::new();
        let permission_context =
            permission_context(postgres_label).with_resource_name(ctx.resource_id.to_string());
        let grant_plan = generator
            .generate_grant_plan(
                &permission_set,
                BindingTarget::Resource,
                &permission_context,
            )
            .context(ErrorData::OperationNotSupported {
                operation: "generate Azure Postgres management grants".to_string(),
                reason: format!(
                    "failed to generate management grants for permission set '{}'",
                    permission_set.id
                ),
            })?;

        for (binding_index, binding) in grant_plan.bindings.iter().enumerate() {
            let role_definition_id = match &binding.role_definition {
                AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                    expr::template(role_definition_id.clone())
                }
                AzureRoleDefinitionRef::Custom { key } => {
                    let index = grant_plan
                        .custom_roles
                        .iter()
                        .position(|role| role.key == *key)
                        .unwrap_or(0);
                    let role_label = setup_management_role_label(&binding.role_name, index);
                    expr::traversal([
                        "azurerm_role_definition",
                        role_label.as_str(),
                        "role_definition_resource_id",
                    ])
                }
            };
            emit_role_assignment(
                fragment,
                ctx.resource_id,
                postgres_label,
                "management",
                binding_index,
                &binding.role_name,
                role_definition_id,
                principal_id_expr.clone(),
                expr::traversal(["azurerm_postgresql_flexible_server", postgres_label, "id"]),
            );
        }
    }

    Ok(())
}

/// The password is the only Key Vault object database access authorizes.
fn emit_password_read_assignment(
    ctx: &EmitContext<'_>,
    postgres_label: &str,
    principal_label: &str,
    principal_id: Expression,
    fragment: &mut TfFragment,
) -> Result<()> {
    let vault = vault_label(ctx)?;
    let role_id = alien_permissions::generators::azure_runtime::azure_predefined_role_id(
        "Key Vault Secrets User",
    )
    .ok_or_else(|| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: "grant Azure database password access".to_string(),
            reason: "Key Vault Secrets User role is not registered".to_string(),
        })
    })?;
    let role_definition = expr::template(format!(
        "/subscriptions/${{var.azure_subscription_id}}/providers/Microsoft.Authorization/roleDefinitions/{role_id}"
    ));
    let secret_scope = expr::template(format!(
        "${{azurerm_key_vault.{vault}.id}}/secrets/${{azurerm_key_vault_secret.{postgres_label}.name}}"
    ));
    emit_role_assignment(
        fragment,
        ctx.resource_id,
        postgres_label,
        principal_label,
        0,
        "Key Vault Secrets User",
        role_definition,
        principal_id,
        secret_scope,
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_role_assignment(
    fragment: &mut TfFragment,
    postgres_id: &str,
    postgres_label: &str,
    principal_label: &str,
    binding_index: usize,
    role_name: &str,
    role_definition_id: Expression,
    principal_id_expr: Expression,
    scope: Expression,
) {
    let role_label = sanitize_role_label(role_name);
    fragment.resource_blocks.push(resource_block(
        "azurerm_role_assignment",
        &format!("{postgres_label}_{role_label}_{principal_label}_assignment_{binding_index}"),
        [
            attr(
                "name",
                expr::raw(&format!(
                    "uuidv5(\"oid\", \"deployment:azure:postgres-role-assign:${{local.resource_prefix}}:{postgres_id}:{role_label}:{principal_label}:{binding_index}\")"
                )),
            ),
            attr("scope", scope),
            attr("role_definition_id", role_definition_id),
            attr("principal_id", principal_id_expr),
        ],
    ));
}

fn sanitize_role_label(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn postgres_permission_owners<'a>(
    ctx: &'a EmitContext<'_>,
) -> Vec<(String, Vec<&'a PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = resource_permission_refs(profile, ctx.resource_id);
        if refs.is_empty() {
            continue;
        }
        owners.push((profile_name.clone(), refs));
    }
    owners
}

fn management_permission_refs<'a>(ctx: &'a EmitContext<'_>) -> Vec<&'a PermissionSetReference> {
    let Some(profile) = ctx.stack.management().profile() else {
        return Vec::new();
    };
    resource_permission_refs(profile, ctx.resource_id)
}

fn resource_permission_refs<'a>(
    profile: &'a PermissionProfile,
    resource_id: &str,
) -> Vec<&'a PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref);
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with("postgres/"))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref);
            }
        }
    }

    refs
}

fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}
