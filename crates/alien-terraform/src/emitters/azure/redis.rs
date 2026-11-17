//! Azure Redis: Azure Managed Redis emitter, private-only through a private endpoint.
//!
//! Mirrors the runtime Redis controller so the setup
//! package and the live controller converge on the same cloud resources: a `NoCluster`
//! database (TLS only, access-key authentication), public network access disabled, a private
//! endpoint in the network's dedicated subnet, and the `privatelink.redis.azure.net` zone with
//! a VNet link and a zone group.
//!
//! The access key stays inside Azure: the binding names the database, and workloads read the
//! key with `listKeys` (granted by `redis/data-access`). The key is a computed attribute of the
//! provider resource, so it is in Terraform state (sensitive), but nothing here exports it.
//!
//! The private DNS zone and its VNet link exist once per stack. With several Redis resources the
//! first (by id) owns them and the others reference its blocks.

use crate::emitters::azure::cmek::grant_store_key;
use alien_core::{
    import::EmitContext, ErrorData, NetworkSettings, Redis, RedisEvictionPolicy, Result,
};
use alien_error::{AlienError, Context};
use alien_permissions::{
    generators::{AzureRoleDefinitionRef, AzureRuntimePermissionsGenerator},
    BindingTarget,
};
use crate::block::{attr, block, data_block, nested, resource_block};
use crate::emitters::azure::helpers::{
    downcast, emit_profile_grants, permission_context, remote_stack_management_label,
    required_label, tags, ProfileGrants,
};
use crate::expr;
use crate::{TfEmitter, TfFragment};
use hcl::expr::Expression;

const REDIS_PORT: i64 = 10000;
const PRIVATE_DNS_ZONE: &str = "privatelink.redis.azure.net";
const PRIVATE_LINK_GROUP_ID: &str = "redisEnterprise";
/// Must equal the controller's `RDB_FREQUENCY`.
const RDB_FREQUENCY: &str = "6h";

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureRedisEmitter;

/// Azure spells eviction policies in PascalCase. Must match the controller's mapping.
fn azure_eviction_policy(policy: RedisEvictionPolicy) -> &'static str {
    match policy {
        RedisEvictionPolicy::NoEviction => "NoEviction",
        RedisEvictionPolicy::AllKeysLru => "AllKeysLRU",
        RedisEvictionPolicy::AllKeysLfu => "AllKeysLFU",
        RedisEvictionPolicy::AllKeysRandom => "AllKeysRandom",
        RedisEvictionPolicy::VolatileLru => "VolatileLRU",
        RedisEvictionPolicy::VolatileLfu => "VolatileLFU",
        RedisEvictionPolicy::VolatileRandom => "VolatileRandom",
        RedisEvictionPolicy::VolatileTtl => "VolatileTTL",
    }
}

enum PeNetwork {
    Create,
    Byo { pe_subnet_name: String },
}

fn pe_network(ctx: &EmitContext<'_>) -> Result<PeNetwork> {
    let settings = ctx
        .stack
        .resources()
        .find_map(|(_, entry)| entry.config.downcast_ref::<alien_core::Network>())
        .map(|network| &network.settings)
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit Azure Redis setup".to_string(),
                reason: "requires a network resource, but none was found in the stack".to_string(),
            })
        })?;
    match settings {
        NetworkSettings::Create { .. } | NetworkSettings::UseDefault => Ok(PeNetwork::Create),
        NetworkSettings::ByoVnetAzure {
            private_endpoint_subnet_name,
            ..
        } => Ok(PeNetwork::Byo {
            pe_subnet_name: private_endpoint_subnet_name.clone().ok_or_else(|| {
                AlienError::new(ErrorData::OperationNotSupported {
                    operation: "emit Azure Redis setup".to_string(),
                    reason: "a bring-your-own VNet hosting Redis must set \
                             `private_endpoint_subnet_name`: the private endpoint needs a \
                             dedicated subnet, and Alien will not carve one from a VNet it does \
                             not own"
                        .to_string(),
                })
            })?,
        }),
        NetworkSettings::ByoVpcAws { .. } | NetworkSettings::ByoVpcGcp { .. } => {
            Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit Azure Redis setup".to_string(),
                reason: "the stack network is a non-Azure bring-your-own VPC".to_string(),
            }))
        }
    }
}

fn network_label<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
    ctx.stack
        .resources()
        .find(|(_, entry)| entry.config.resource_type() == alien_core::Network::RESOURCE_TYPE)
        .and_then(|(id, _)| ctx.name_for(id))
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit Azure Redis setup".to_string(),
                reason: "requires a network resource, but none was found in the stack".to_string(),
            })
        })
}

/// Fixed Terraform addresses for the shared zone and link, so they do not move when a Redis
/// whose id sorts first is added.
///
/// Postgres labels its zone and link `<id>_dns_zone` and `<id>_dns_link`, so a Postgres with the
/// id `redis-shared` would take `redis_shared_dns_zone`. These labels end in `_zone` and `_link`,
/// which no Postgres label does.
const SHARED_ZONE_LABEL: &str = "alien_redis_shared_zone";
const SHARED_LINK_LABEL: &str = "alien_redis_shared_link";

/// Label of the Redis resource that owns the stack's single private DNS zone and VNet link.
fn zone_owner_label<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
    ctx.stack
        .resources()
        // Live resources are not emitted, so only a frozen Redis can own the shared object.
        .filter(|(_, entry)| {
            entry.config.resource_type() == Redis::RESOURCE_TYPE
                && entry.lifecycle == alien_core::ResourceLifecycle::Frozen
        })
        .map(|(id, _)| id)
        .min()
        .and_then(|id| ctx.name_for(id))
        .ok_or_else(|| {
            AlienError::new(ErrorData::GenericError {
                message: "no Redis resource found for the private DNS zone owner".to_string(),
            })
        })
}

impl TfEmitter for AzureRedisEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let redis = downcast::<Redis>(ctx, Redis::RESOURCE_TYPE)?;
        let (sku, _) = redis.azure_managed_redis_sku()?;
        if redis.persistence && !redis.high_availability {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit Azure Redis setup".to_string(),
                reason: "persistence needs highAvailability(): Azure Managed Redis only persists \
                         data on a high availability database"
                    .to_string(),
            }));
        }
        let label = required_label(ctx)?;
        let network_label = network_label(ctx)?.to_string();
        let network = pe_network(ctx)?;
        let byo = matches!(network, PeNetwork::Byo { .. });
        let owns_zone = zone_owner_label(ctx)? == label;
        let pe_subnet_label = format!("{label}_pe_subnet");
        let pe_label = format!("{label}_pe");
        let zone_label = SHARED_ZONE_LABEL.to_string();
        let mut fragment = TfFragment::default();

        let mut database = vec![
            attr(
                "client_protocol",
                Expression::String("Encrypted".to_string()),
            ),
            attr(
                "clustering_policy",
                Expression::String("NoCluster".to_string()),
            ),
            attr(
                "eviction_policy",
                Expression::String(azure_eviction_policy(redis.eviction_policy).to_string()),
            ),
            attr("access_keys_authentication_enabled", Expression::Bool(true)),
        ];
        if redis.persistence {
            database.push(attr(
                "persistence_redis_database_backup_frequency",
                Expression::String(RDB_FREQUENCY.to_string()),
            ));
        }
        let mut managed_redis = vec![
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{}", redis.id)),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr("sku_name", Expression::String(sku.to_string())),
            attr(
                "high_availability_enabled",
                Expression::Bool(redis.high_availability),
            ),
            attr(
                "public_network_access",
                Expression::String("Disabled".to_string()),
            ),
            attr("tags", tags(ctx, "redis")),
            nested(block("default_database", database)),
        ];
        // A customer key encrypts the cache's data at rest. Managed Redis accepts it only at
        // create, through a user-assigned identity that may use the key.
        if let Some(key) = redis.encryption_key.as_ref() {
            let store_key = grant_store_key(ctx, &mut fragment, key, label, "redis-encryption")?;
            managed_redis.push(nested(store_key.identity_block));
            managed_redis.push(nested(block(
                "customer_managed_key",
                [
                    attr("key_vault_key_id", store_key.versioned_key_id),
                    attr("user_assigned_identity_id", store_key.identity_id),
                ],
            )));
            managed_redis.push(attr(
                "depends_on",
                expr::raw(format!("[{}]", store_key.role_dependency)),
            ));
        }
        fragment.resource_blocks.push(resource_block(
            "azurerm_managed_redis",
            label,
            managed_redis,
        ));

        match &network {
            PeNetwork::Create => {
                // The network emitter already carves the endpoint subnet in a managed VNet, so
                // reuse it. Adding another subnet here would collide with it or with the
                // Postgres emitter's own endpoint subnet.
            }
            PeNetwork::Byo { pe_subnet_name } => {
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

        if owns_zone {
            fragment.resource_blocks.push(resource_block(
                "azurerm_private_dns_zone",
                &zone_label,
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
                SHARED_LINK_LABEL,
                [
                    attr(
                        "name",
                        expr::template("${local.resource_prefix}-redis-dns-link".to_string()),
                    ),
                    attr(
                        "resource_group_name",
                        expr::raw("var.azure_resource_group_name"),
                    ),
                    attr(
                        "private_dns_zone_name",
                        expr::traversal(["azurerm_private_dns_zone", &zone_label, "name"]),
                    ),
                    attr(
                        "virtual_network_id",
                        if byo {
                            expr::traversal([
                                "data",
                                "azurerm_virtual_network",
                                &network_label,
                                "id",
                            ])
                        } else {
                            expr::traversal(["azurerm_virtual_network", &network_label, "id"])
                        },
                    ),
                    // The zone group registers the endpoint's record.
                    attr("registration_enabled", Expression::Bool(false)),
                ],
            ));
        }

        let mut endpoint = vec![
            attr(
                "name",
                expr::template(format!("${{local.resource_prefix}}-{}-pe", redis.id)),
            ),
            attr(
                "resource_group_name",
                expr::raw("var.azure_resource_group_name"),
            ),
            attr("location", expr::raw("var.azure_location")),
            attr(
                "subnet_id",
                if byo {
                    expr::traversal(["data", "azurerm_subnet", &pe_subnet_label, "id"])
                } else {
                    expr::traversal(["azurerm_subnet", &format!("{network_label}_pe"), "id"])
                },
            ),
            nested(block(
                "private_service_connection",
                [
                    attr(
                        "name",
                        expr::template(format!("${{local.resource_prefix}}-{}-psc", redis.id)),
                    ),
                    attr("is_manual_connection", Expression::Bool(false)),
                    attr(
                        "private_connection_resource_id",
                        expr::traversal(["azurerm_managed_redis", label, "id"]),
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
                            &zone_label,
                            "id",
                        ])]),
                    ),
                ],
            )),
        ];
        if !owns_zone {
            // The link belongs to the owner; the endpoint only needs the zone to exist first.
            endpoint.push(attr(
                "depends_on",
                expr::raw(format!(
                    "[azurerm_private_dns_zone_virtual_network_link.{SHARED_LINK_LABEL}]"
                )),
            ));
        }
        fragment.resource_blocks.push(resource_block(
            "azurerm_private_endpoint",
            &pe_label,
            endpoint,
        ));

        // Workloads get `redis/data-access` (listKeys on this database); management gets
        // `redis/heartbeat` (Reader and Monitoring Reader). Never the ownership set.
        emit_profile_grants(
            ctx,
            &mut fragment,
            label,
            ProfileGrants {
                resource_type: "redis",
                context: permission_context(label)
                    .with_resource_name(format!("${{azurerm_managed_redis.{label}.name}}")),
                scope: None,
            },
        )?;
        emit_management_grants(ctx, label, &mut fragment)?;

        fragment.retain_resources(|kind, name| {
            kind == "azurerm_managed_redis"
                || (kind == "azurerm_user_assigned_identity"
                    && name == format!("{label}_encryption"))
                || (kind == "azurerm_role_assignment" && name == format!("{label}_encryption_key"))
        });
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let redis = downcast::<Redis>(ctx, Redis::RESOURCE_TYPE)?;
        let (sku, _) = redis.azure_managed_redis_sku()?;
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "clusterName",
                expr::traversal(["azurerm_managed_redis", label, "name"]),
            ),
            (
                "clusterResourceId",
                expr::traversal(["azurerm_managed_redis", label, "id"]),
            ),
            ("resourceGroup", expr::raw("var.azure_resource_group_name")),
            (
                "privateEndpointName",
                expr::traversal(["azurerm_private_endpoint", &format!("{label}_pe"), "name"]),
            ),
            (
                "host",
                expr::traversal(["azurerm_managed_redis", label, "hostname"]),
            ),
            ("port", Expression::Number(hcl::Number::from(REDIS_PORT))),
            ("sku", Expression::String(sku.to_string())),
            (
                "highAvailability",
                Expression::Bool(redis.high_availability),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            (
                "service",
                Expression::String("azure-managed-redis".to_string()),
            ),
            (
                "host",
                expr::traversal(["azurerm_managed_redis", label, "hostname"]),
            ),
            ("port", Expression::Number(hcl::Number::from(REDIS_PORT))),
            ("username", Expression::String("default".to_string())),
            // The workload lists the key from this database at load time.
            (
                "databaseResourceId",
                expr::template(format!(
                    "${{azurerm_managed_redis.{label}.id}}/databases/default"
                )),
            ),
        ])))
    }
}

/// `redis/heartbeat` for the management identity: predefined read roles on this cluster.
fn emit_management_grants(
    ctx: &EmitContext<'_>,
    label: &str,
    fragment: &mut TfFragment,
) -> Result<()> {
    let Some(management_label) = remote_stack_management_label(ctx) else {
        return Ok(());
    };
    let Some(profile) = ctx.stack.management().profile() else {
        return Ok(());
    };
    let mut refs = Vec::new();
    for key in [ctx.resource_id, "*"] {
        if let Some(entries) = profile.0.get(key) {
            refs.extend(
                entries
                    .iter()
                    .filter(|entry| entry.id().starts_with("redis/")),
            );
        }
    }
    let mut seen = std::collections::HashSet::new();
    for permission_ref in refs {
        if !seen.insert(permission_ref.id().to_string()) {
            continue;
        }
        let Some(permission_set) =
            permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
        else {
            continue;
        };
        if permission_set.id.ends_with("/provision") || permission_set.platforms.azure.is_none() {
            continue;
        }
        let context = permission_context(label)
            .with_resource_name(format!("${{azurerm_managed_redis.{label}.name}}"));
        let plan = AzureRuntimePermissionsGenerator::new()
            .generate_grant_plan(&permission_set, BindingTarget::Resource, &context)
            .context(ErrorData::GenericError {
                message: format!(
                    "failed to generate Azure Redis management grants for '{}'",
                    permission_set.id
                ),
            })?;
        for (index, binding) in plan.bindings.iter().enumerate() {
            let AzureRoleDefinitionRef::Predefined { role_definition_id } =
                &binding.role_definition
            else {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "emit Azure Redis management grants".to_string(),
                    reason: format!(
                        "permission set '{}' needs a custom role, which this emitter does not create",
                        permission_set.id
                    ),
                }));
            };
            let role_label: String = binding
                .role_name
                .chars()
                .map(|ch| {
                    if ch.is_ascii_alphanumeric() {
                        ch.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect();
            fragment.resource_blocks.push(resource_block(
                "azurerm_role_assignment",
                &format!("{label}_{role_label}_management_assignment_{index}"),
                [
                    attr(
                        "name",
                        expr::raw(&format!(
                            "uuidv5(\"oid\", \"deployment:azure:redis-role-assign:${{local.resource_prefix}}:{}:{role_label}:management:{index}\")",
                            ctx.resource_id
                        )),
                    ),
                    attr(
                        "scope",
                        expr::traversal(["azurerm_managed_redis", label, "id"]),
                    ),
                    attr("role_definition_id", expr::template(role_definition_id.clone())),
                    attr(
                        "principal_id",
                        expr::traversal([
                            "azurerm_user_assigned_identity",
                            management_label,
                            "principal_id",
                        ]),
                    ),
                ],
            ));
        }
    }
    Ok(())
}
