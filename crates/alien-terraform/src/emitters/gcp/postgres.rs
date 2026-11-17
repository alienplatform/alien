//! GCP Postgres — Cloud SQL (Enterprise) instance, private-only via Private Service Connect.
//!
//! Mirrors the live-resource create flow in
//! runtime controller so the setup package (push
//! deployment) and the controller (pull deployment) converge on the same cloud
//! resources:
//!
//!   1. A generated master password in Secret Manager. The import/binding refs carry only the
//!      secret name. Secret values use ephemeral reads and write-only arguments.
//!   2. The Cloud SQL instance — `ipv4_enabled = false` + `psc_config` so it is
//!      private-only on the stack VPC and never publicly reachable.
//!   3. The PSC consumer endpoint: a regional internal `google_compute_address`
//!      in the stack subnetwork plus a regional `google_compute_forwarding_rule`
//!      whose `target` is the instance's generated PSC service-attachment link.
//!      That address's internal IP is the binding `host`.
//!   4. The default database and the `alien` SQL user (password from the secret).
//!   5. `postgres/data-access` + `postgres/management` IAM — Secret Manager read for
//!      the workload service accounts, instance describe/modify for management.

use crate::emitters::gcp::cmek::{grant_store_key, is_store_key_grant};
use crate::emitters::postgres_credentials::{
    database_revision, generated_password, password_revision,
};
use alien_core::{
    import::EmitContext, ErrorData, Network, PermissionProfile, PermissionSetReference, Postgres,
    RemoteStackManagement, Result, ServiceAccount,
};
use alien_error::AlienError;
use alien_permissions::BindingTarget;
use crate::block::ephemeral_block;
use crate::block::{attr, block, nested, resource_block};
use crate::emitters::gcp::helpers::{
    downcast, emit_custom_role_and_bindings_for_target, permission_context, required_label,
    resource_prefix_template, service_account_member_for_label,
};
use crate::expr;
use crate::{TfEmitter, TfFragment};
use hcl::expr::Expression;

/// Port Postgres listens on; fixed for Cloud SQL.
const POSTGRES_PORT: i64 = 5432;
/// Master username; fixed across backends and what the binding/import advertise.
const MASTER_USERNAME: &str = alien_core::POSTGRES_ADMIN_USERNAME;
/// Default database name created on the instance, from the single source of truth
/// `alien_core::POSTGRES_DATABASE_NAME` — shared with the controller, importer, and the other
/// emitters so a frozen import always binds to a database that exists.
const DATABASE_NAME: &str = alien_core::POSTGRES_DATABASE_NAME;

/// The discrete `db-custom-*` Enterprise tiers Cloud SQL accepts, as
/// `(cpu_cap_vcpu, mem_cap_gib, tier_name)` ascending. MUST stay byte-identical to the controller's
/// the runtime controller's `GCP_POSTGRES_TIERS`: the push (this emitter) and pull
/// (controller) paths both select from this same list, so any divergence is a
/// push-succeeds/runtime-fails split-brain (a cross-crate parity test pins them equal). The
/// `db-perf-optimized-*` tiers are ENTERPRISE_PLUS-only, so none appear here.
const GCP_POSTGRES_TIERS: &[(f64, f64, &str)] = &[
    (1.0, 3.75, "db-custom-1-3840"),
    (2.0, 7.5, "db-custom-2-7680"),
    (4.0, 15.0, "db-custom-4-15360"),
    (8.0, 30.0, "db-custom-8-30720"),
    (16.0, 60.0, "db-custom-16-61440"),
];

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpPostgresEmitter;

impl TfEmitter for GcpPostgresEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let postgres = downcast::<Postgres>(ctx, Postgres::RESOURCE_TYPE)?;
        let label = required_label(ctx)?;
        let mut fragment = TfFragment::default();

        // The PSC consumer endpoint lives in the stack VPC. Resolve the network + subnetwork
        // self-links from the network resource's label into per-resource locals (the network
        // emitter exposes them through a network-mode ternary, not a fixed local), then bind the
        // consumer address + forwarding rule to those locals.
        let network_label = network_label(ctx)?;
        let network_self_link_local = format!("{label}_network_self_link");
        let subnetwork_self_link_local = format!("{label}_subnetwork_self_link");
        fragment.locals.insert(
            network_self_link_local.clone(),
            network_attribute_expr(ctx, network_label, false)?,
        );
        fragment.locals.insert(
            subnetwork_self_link_local.clone(),
            network_attribute_expr(ctx, network_label, true)?,
        );
        let network_self_link = expr::raw(format!("local.{network_self_link_local}"));
        let subnetwork_self_link = expr::raw(format!("local.{subnetwork_self_link_local}"));

        // 1. Persist the generated password only in Secret Manager, then reopen
        //    that exact version for the SQL user, including after interrupted applies.
        let revision = password_revision(&mut fragment, label);
        fragment.ephemeral_blocks.push(ephemeral_block(
            "google_secret_manager_secret_version",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "secret",
                    expr::traversal(["google_secret_manager_secret", label, "secret_id"]),
                ),
                attr(
                    "version",
                    expr::traversal(["google_secret_manager_secret_version", label, "version"]),
                ),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "google_secret_manager_secret",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "secret_id",
                    resource_prefix_template(&format!("{}-credentials", postgres.id)),
                ),
                // Automatic replication mirrors the controller's `AutomaticReplication`.
                nested(block("replication", [nested(block("auto", []))])),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "google_secret_manager_secret_version",
            label,
            [
                attr(
                    "secret",
                    expr::traversal(["google_secret_manager_secret", label, "id"]),
                ),
                attr("secret_data_wo", generated_password(label)),
                attr("secret_data_wo_version", revision),
                attr("deletion_policy", "ABANDON".into()),
                nested(block(
                    "lifecycle",
                    [attr("create_before_destroy", true.into())],
                )),
            ],
        ));

        // 2. The Cloud SQL instance — private-only: ipv4_enabled false + PSC enabled, with the
        //    stack project as the sole allowed PSC consumer.
        // A customer key encrypts the instance's disks, backups and logs. Cloud SQL accepts it
        // only at create, so the instance waits for the service agent's grant.
        let store_key = postgres
            .encryption_key
            .as_ref()
            .map(|key| grant_store_key(ctx, &mut fragment, key, label, "sqladmin.googleapis.com"))
            .transpose()?;
        let mut instance_attrs = vec![
            attr("project", expr::raw("var.gcp_project")),
            attr("name", resource_prefix_template(&postgres.id)),
            attr("region", expr::raw("var.gcp_region")),
            attr(
                "database_version",
                Expression::String(database_version(postgres)),
            ),
            // Retain on uninstall; the recovery workflow detaches the instance.
            attr("deletion_protection", Expression::Bool(true)),
            nested(block(
                "settings",
                [
                    attr("tier", Expression::String(gcp_postgres_tier(postgres)?)),
                    // The provider's grow-only diff (google >= 7.4) preserves any larger
                    // auto-grown disk, without ignore_changes hiding requested growth.
                    attr(
                        "disk_size",
                        Expression::Number(hcl::Number::from(
                            postgres.requested_storage_gib()?.max(10),
                        )),
                    ),
                    attr("disk_autoresize", Expression::Bool(true)),
                    attr("edition", Expression::String("ENTERPRISE".to_string())),
                    attr(
                        "availability_type",
                        Expression::String(
                            if postgres.high_availability {
                                "REGIONAL"
                            } else {
                                "ZONAL"
                            }
                            .to_string(),
                        ),
                    ),
                    nested(block(
                        "ip_configuration",
                        [
                            attr("ipv4_enabled", Expression::Bool(false)),
                            nested(block(
                                "psc_config",
                                [
                                    attr("psc_enabled", Expression::Bool(true)),
                                    // Connect by the reserved consumer IP (the binding `host`),
                                    // not a DNS name. Auto-DNS would make the forwarding rule
                                    // register a Service Directory entry under `goog-psc-default`,
                                    // which requires `servicedirectory.services.create` on the
                                    // installer — an extra grant for a name nothing resolves.
                                    attr("psc_auto_dns_enabled", Expression::Bool(false)),
                                    attr(
                                        "allowed_consumer_projects",
                                        Expression::Array(vec![expr::raw("var.gcp_project")]),
                                    ),
                                ],
                            )),
                        ],
                    )),
                    nested(block(
                        "backup_configuration",
                        [
                            attr("enabled", Expression::Bool(true)),
                            attr("point_in_time_recovery_enabled", Expression::Bool(true)),
                        ],
                    )),
                ],
            )),
        ];
        if let Some(store_key) = &store_key {
            instance_attrs.push(attr("encryption_key_name", store_key.name.clone()));
            instance_attrs.push(attr(
                "depends_on",
                expr::raw(format!("[{}]", store_key.grant_dependency)),
            ));
        }
        fragment.resource_blocks.push(resource_block(
            "google_sql_database_instance",
            label,
            instance_attrs,
        ));

        // 3. PSC consumer endpoint in the stack VPC. The forwarding rule targets the instance's
        //    generated `psc_service_attachment_link` (server-assigned, never derived); its IP is
        //    the binding host. `load_balancing_scheme = ""` is required when the target is a
        //    service attachment.
        fragment.resource_blocks.push(resource_block(
            "google_compute_address",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "name",
                    resource_prefix_template(&format!("{}-psc-ip", postgres.id)),
                ),
                attr("region", expr::raw("var.gcp_region")),
                attr("address_type", Expression::String("INTERNAL".to_string())),
                attr("subnetwork", subnetwork_self_link.clone()),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "google_compute_forwarding_rule",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "name",
                    resource_prefix_template(&format!("{}-psc", postgres.id)),
                ),
                attr("region", expr::raw("var.gcp_region")),
                attr("load_balancing_scheme", Expression::String(String::new())),
                attr(
                    "target",
                    expr::traversal([
                        "google_sql_database_instance",
                        label,
                        "psc_service_attachment_link",
                    ]),
                ),
                attr("network", network_self_link),
                attr("subnetwork", subnetwork_self_link),
                attr(
                    "ip_address",
                    expr::traversal(["google_compute_address", label, "id"]),
                ),
            ],
        ));

        // 4. The default database and the `alien` SQL user, password drawn from the secret.
        fragment.resource_blocks.push(resource_block(
            "google_sql_database",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr("name", Expression::String(DATABASE_NAME.to_string())),
                attr(
                    "instance",
                    expr::traversal(["google_sql_database_instance", label, "name"]),
                ),
            ],
        ));
        fragment.resource_blocks.push(resource_block(
            "google_sql_user",
            label,
            [
                // This module owns both instance and role. Runtime databases may depend
                // on the role, so instance deletion removes them together without DROP ROLE.
                // ABANDON is for whole-instance teardown, not standalone credential revocation.
                attr("deletion_policy", Expression::String("ABANDON".to_string())),
                attr("project", expr::raw("var.gcp_project")),
                attr("name", Expression::String(MASTER_USERNAME.to_string())),
                attr(
                    "instance",
                    expr::traversal(["google_sql_database_instance", label, "name"]),
                ),
                attr(
                    "password_wo",
                    expr::traversal([
                        "ephemeral",
                        "google_secret_manager_secret_version",
                        label,
                        "secret_data",
                    ]),
                ),
                attr(
                    "password_wo_version",
                    database_revision(&format!(
                        "google_secret_manager_secret_version.{label}.version"
                    )),
                ),
            ],
        ));

        // 5. postgres/data-access (workload secret read) + postgres/management (instance describe /
        //    modify) IAM.
        emit_postgres_iam(ctx, &mut fragment)?;

        // PSC consumer endpoints are access paths, not database dependencies. Remove
        // them with workload IAM; preserve the database, SQL login and credential so
        // an administrator can reconnect through a newly authorized private endpoint.
        fragment.retain_resources(|kind, _| {
            is_store_key_grant(kind)
                || matches!(
                    kind,
                    "google_sql_database_instance"
                        | "google_sql_database"
                        | "google_sql_user"
                        | "google_secret_manager_secret"
                )
        });
        // Versions rotate by replacement. Guard the parent, not version bookkeeping.
        fragment.retain_state(|kind, _| matches!(kind, "google_secret_manager_secret_version"));
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        // Exactly the `GcpPostgresImportData` field set.
        Ok(expr::object([
            (
                "instanceName",
                expr::traversal(["google_sql_database_instance", label, "name"]),
            ),
            (
                "database",
                expr::traversal(["google_sql_database", label, "name"]),
            ),
            (
                "passwordSecretName",
                expr::traversal(["google_secret_manager_secret", label, "secret_id"]),
            ),
            (
                "pscServiceAttachment",
                expr::traversal([
                    "google_sql_database_instance",
                    label,
                    "psc_service_attachment_link",
                ]),
            ),
            (
                "pscAddressName",
                expr::traversal(["google_compute_address", label, "name"]),
            ),
            (
                "pscForwardingRuleName",
                expr::traversal(["google_compute_forwarding_rule", label, "name"]),
            ),
            (
                "endpointIp",
                expr::traversal(["google_compute_address", label, "address"]),
            ),
            ("region", expr::raw("var.gcp_region")),
            (
                "databaseVersion",
                expr::traversal(["google_sql_database_instance", label, "database_version"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("cloud-sql".to_string())),
            // Host is the PSC consumer endpoint IP — the only path workloads have to the
            // private-only instance.
            (
                "host",
                expr::traversal(["google_compute_address", label, "address"]),
            ),
            ("port", Expression::Number(hcl::Number::from(POSTGRES_PORT))),
            ("database", Expression::String(DATABASE_NAME.to_string())),
            ("username", Expression::String(MASTER_USERNAME.to_string())),
            (
                "passwordSecretName",
                expr::traversal(["google_secret_manager_secret", label, "secret_id"]),
            ),
        ])))
    }
}

/// Cloud SQL `databaseVersion` (e.g. `POSTGRES_16`) from the resource's major version.
fn database_version(postgres: &Postgres) -> String {
    format!("POSTGRES_{}", postgres.version)
}

/// The Enterprise `db-custom-*` tier for `postgres`, honoring requested cpu AND memory: the
/// smallest ladder tier whose vCPU and memory caps both satisfy the request (round up; never
/// under-provision). Unset cpu/memory -> the smallest tier. MUST match
/// `GcpPostgresController::tier`; the cross-crate parity test pins them equal. `pub` so that test
/// (against the runtime controller) can call it directly.
pub fn gcp_postgres_tier(postgres: &Postgres) -> Result<String> {
    let req_cpu = requested_cpu(postgres)?;
    let req_mem = requested_memory_gib(postgres)?;
    GCP_POSTGRES_TIERS
        .iter()
        .find(|(cpu_cap, mem_cap, _)| *cpu_cap >= req_cpu && *mem_cap >= req_mem)
        .map(|(_, _, name)| name.to_string())
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "custom cpu/memory sizing on GCP Cloud SQL Postgres".to_string(),
                reason: "request exceeds the largest tier (max 16 vCPU / 60 GiB)".to_string(),
            })
        })
}

fn requested_cpu(postgres: &Postgres) -> Result<f64> {
    Ok(postgres.requested_cpu()?.unwrap_or(0.0))
}

fn requested_memory_gib(postgres: &Postgres) -> Result<f64> {
    Ok(postgres.requested_memory_gib()?.unwrap_or(0.0))
}

/// The Terraform label of the stack network resource (`default-network`), which the GCP network
/// emitter materializes the VPC + subnetwork under. Fail-fast if it is absent — the consumer
/// endpoint cannot be placed without a network.
pub(crate) fn network_label<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
    ctx.stack
        .resources()
        .find(|(_id, entry)| entry.config.resource_type() == Network::RESOURCE_TYPE)
        .and_then(|(id, _entry)| ctx.name_for(id))
        .ok_or_else(|| {
            AlienError::new(alien_core::ErrorData::OperationNotSupported {
                operation: "emit GCP private endpoint setup".to_string(),
                reason: "requires a network resource, but none was found in the stack".to_string(),
            })
        })
}

/// Resolve the network emitter's actual addresses for dynamic, default and BYO networks.
pub(crate) fn network_attribute_expr(
    ctx: &EmitContext<'_>,
    label: &str,
    subnet: bool,
) -> Result<Expression> {
    let network = ctx
        .stack
        .resources()
        .find_map(|(_, entry)| entry.config.downcast_ref::<alien_core::Network>())
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit GCP private endpoint setup".to_string(),
                reason: "requires a network resource".to_string(),
            })
        })?;
    match &network.settings {
        alien_core::NetworkSettings::Create { .. } => {
            let (resource_type, created_label, existing_label) = if subnet {
                (
                    "google_compute_subnetwork",
                    format!("{label}_workload"),
                    format!("{label}_existing_subnet"),
                )
            } else {
                (
                    "google_compute_network",
                    label.to_string(),
                    label.to_string(),
                )
            };
            // The mode follows the cluster mode on a Kubernetes target: an existing cluster's
            // network is looked up, never created.
            let mode =
                crate::emitters::gcp::network::network_mode_expr(ctx.targets_kubernetes);
            Ok(expr::raw(format!(
                "{mode} == \"create-new\" ? {resource_type}.{created_label}[0].self_link : data.{resource_type}.{existing_label}[0].self_link"
            )))
        }
        alien_core::NetworkSettings::UseDefault | alien_core::NetworkSettings::ByoVpcGcp { .. } => {
            let (resource_type, label) = if subnet {
                ("google_compute_subnetwork", format!("{label}_subnet"))
            } else {
                ("google_compute_network", label.to_string())
            };
            Ok(expr::traversal([
                "data",
                resource_type,
                &label,
                "self_link",
            ]))
        }
        alien_core::NetworkSettings::ByoVpcAws { .. }
        | alien_core::NetworkSettings::ByoVnetAzure { .. } => {
            Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit GCP private endpoint setup".to_string(),
                reason: "requires a GCP network".to_string(),
            }))
        }
    }
}

/// Emit GCP custom roles + IAM bindings for every `postgres/*` permission set on each owner
/// (workload service accounts + management). `/provision` sets are skipped (setup-time only),
/// matching the vault emitter; `BindingTarget::Resource` scopes each grant to this instance's
/// secret / instance via the permission-set conditions, with `resourceName` set to the
/// stack-prefixed resource id (the secret is `${prefix}-<id>-credentials`).
fn emit_postgres_iam(ctx: &EmitContext<'_>, fragment: &mut TfFragment) -> Result<()> {
    let postgres = downcast::<Postgres>(ctx, Postgres::RESOURCE_TYPE)?;
    emit_family_iam(ctx, fragment, "postgres", &postgres.id)
}

/// Shared by every GCP data resource whose permission sets are named `<family>/*`: grants each
/// set to the workload service accounts and management, scoped by the stack-prefixed resource id.
pub(crate) fn emit_family_iam(
    ctx: &EmitContext<'_>,
    fragment: &mut TfFragment,
    family: &str,
    id: &str,
) -> Result<()> {
    let resource_label = required_label(ctx)?;
    let resource_name = format!("${{local.resource_prefix}}-{id}");

    for (owner_label, permission_refs) in permission_owners(ctx, family) {
        let member = service_account_member_for_label(&owner_label);
        let role_owner_label = format!("{owner_label}_{resource_label}");
        let context = permission_context(&owner_label, ctx.stack.id())
            .with_resource_name(resource_name.clone());

        for permission_ref in permission_refs {
            let Some(permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            if !permission_set.id.starts_with(&format!("{family}/"))
                || permission_set.id.ends_with("/provision")
            {
                continue;
            }

            let binding_owner_label = binding_owner_label(&role_owner_label, &permission_set.id);
            emit_custom_role_and_bindings_for_target(
                fragment,
                &binding_owner_label,
                &member,
                &permission_set,
                &context,
                BindingTarget::Resource,
            )?;
        }
    }

    Ok(())
}

/// Owners that receive `postgres/*` grants: one per workload permission profile (`<profile>-sa`)
/// plus management. Mirrors the AWS postgres emitter's `postgres_permission_owners`.
fn permission_owners(
    ctx: &EmitContext<'_>,
    family: &str,
) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = permission_refs(profile, ctx.resource_id, family);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if let Some(label) = service_account_label_for_id(ctx, &service_account_id) {
            owners.push((label.to_string(), refs));
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = permission_refs(profile, ctx.resource_id, family);
        if !refs.is_empty() {
            if let Some(label) = remote_stack_management_label(ctx) {
                owners.push((label.to_string(), refs));
            }
        }
    }

    owners
}

/// Collect the `postgres/*` permission references for `resource_id` (direct refs plus
/// `*`-wildcard refs). Mirrors the AWS postgres emitter's `postgres_permission_refs`.
fn permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
    family: &str,
) -> Vec<PermissionSetReference> {
    let mut refs = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    if let Some(resource_refs) = profile.0.get(resource_id) {
        for permission_ref in resource_refs {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    if let Some(wildcard_refs) = profile.0.get("*") {
        for permission_ref in wildcard_refs
            .iter()
            .filter(|permission_ref| permission_ref.id().starts_with(&format!("{family}/")))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    refs
}

fn service_account_label_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<&'a str> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()?;
    ctx.name_for(service_account_id)
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

/// A Terraform-label-safe owner+permission-set label. Mirrors the vault emitter's helper.
fn binding_owner_label(owner_label: &str, permission_set_id: &str) -> String {
    format!(
        "{owner_label}_{}",
        permission_set_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() {
                    ch.to_ascii_lowercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
    )
}
