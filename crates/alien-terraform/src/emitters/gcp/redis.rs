//! GCP Redis: Memorystore for Valkey instance, private-only through Private Service Connect.
//!
//! Mirrors the runtime Redis controller so the setup
//! package and the controller converge on the same cloud resources:
//!
//!   1. A service connection policy for the `gcp-memorystore` service class, once per stack and
//!      network. The Redis resource with the smallest id owns it.
//!   2. The instance: cluster mode disabled, token auth, TLS, one shard, private endpoint
//!      through PSC auto-connection in the stack network.
//!   3. `redis/data-access` (workloads read the instance's token from Memorystore, conditioned
//!      to this instance) and `redis/heartbeat` (management reads the instance) grants. No
//!      secret: Memorystore generates the token and only its API returns it.

use super::postgres::{emit_family_iam, network_attribute_expr, network_label};
use crate::emitters::gcp::cmek::{grant_store_key, is_store_key_grant};
use alien_core::{import::EmitContext, ErrorData, Redis, RedisEvictionPolicy, Result};
use alien_error::AlienError;
use crate::block::{attr, block, nested, resource_block};
use crate::emitters::gcp::helpers::{
    downcast, labels, required_label, resource_prefix_template,
};
use crate::expr;
use crate::{TfEmitter, TfFragment};
use hcl::expr::Expression;

const REDIS_PORT: i64 = 6379;
/// Same value as the controller's `ENGINE_VERSION`.
const ENGINE_VERSION: &str = "VALKEY_9_0";
const SERVICE_CLASS: &str = "gcp-memorystore";
/// The shared policy has a fixed Terraform address and variable name. They must not depend on
/// which Redis owns it, or adding a Redis whose id sorts first would make Terraform replace it.
const SHARED_POLICY_LABEL: &str = "redis_shared_psc_policy";
const SHARED_POLICY_CREATE_VAR: &str = "redis_shared_create_psc_policy";
const RDB_SNAPSHOT_PERIOD: &str = "SIX_HOURS";

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpRedisEmitter;

/// Label of the Redis resource that owns the stack's single service connection policy.
fn policy_owner_label<'a>(ctx: &'a EmitContext<'_>) -> Result<&'a str> {
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
                message: "no Redis resource found for the service connection policy owner"
                    .to_string(),
            })
        })
}

impl TfEmitter for GcpRedisEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let redis = downcast::<Redis>(ctx, Redis::RESOURCE_TYPE)?;
        let (node_type, _) = redis.memorystore_node_type()?;
        if redis.persistence && !redis.high_availability {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "emit GCP Redis setup".to_string(),
                reason: "persistence needs highAvailability(): the replica is what keeps the \
                         RDB snapshots across a node failure"
                    .to_string(),
            }));
        }
        let label = required_label(ctx)?;
        let network_label = network_label(ctx)?;
        let owner = policy_owner_label(ctx)?;
        let owns_policy = owner == label;
        let mut fragment = TfFragment::default();

        // Memorystore and the connection policy want `projects/p/global/networks/n`, not the
        // full compute URL that `self_link` returns (the API rejects the URL form).
        let network_local = format!("{label}_network_path");
        let subnetwork_local = format!("{label}_subnetwork_path");
        for (local, subnet) in [(&network_local, false), (&subnetwork_local, true)] {
            let self_link = network_attribute_expr(ctx, network_label, subnet)?;
            fragment.locals.insert(
                local.clone(),
                expr::raw(format!(
                    "trimprefix({}, \"https://www.googleapis.com/compute/v1/\")",
                    expr_to_hcl(&self_link)?
                )),
            );
        }

        if owns_policy {
            // GCP allows one Memorystore service connection policy per network and region. A
            // VPC that already has one (from another stack or by hand) cannot get a second, so
            // the operator turns this off and the existing policy serves the instances.
            let create = SHARED_POLICY_CREATE_VAR.to_string();
            fragment.variable_blocks.push(
                hcl::Block::builder("variable")
                    .add_label(create.clone())
                    .add_attribute(("type", expr::raw("bool")))
                    .add_attribute(("default", true))
                    .add_attribute(("nullable", false))
                    .add_attribute((
                        "description",
                        "Set to false when the network already has a Memorystore service connection policy in this region. GCP allows only one.",
                    ))
                    .build(),
            );
            fragment.resource_blocks.push(resource_block(
                "google_network_connectivity_service_connection_policy",
                SHARED_POLICY_LABEL,
                [
                    attr("count", expr::raw(format!("var.{create} ? 1 : 0"))),
                    attr("project", expr::raw("var.gcp_project")),
                    attr("location", expr::raw("var.gcp_region")),
                    attr("name", resource_prefix_template("redis-psc")),
                    attr(
                        "service_class",
                        Expression::String(SERVICE_CLASS.to_string()),
                    ),
                    attr("network", expr::raw(format!("local.{network_local}"))),
                    nested(block(
                        "psc_config",
                        [attr(
                            "subnetworks",
                            Expression::Array(vec![expr::raw(format!("local.{subnetwork_local}"))]),
                        )],
                    )),
                    attr("labels", labels(ctx, "redis")),
                ],
            ));
        }

        let persistence = if redis.persistence {
            nested(block(
                "persistence_config",
                [
                    attr("mode", Expression::String("RDB".to_string())),
                    nested(block(
                        "rdb_config",
                        [attr(
                            "rdb_snapshot_period",
                            Expression::String(RDB_SNAPSHOT_PERIOD.to_string()),
                        )],
                    )),
                ],
            ))
        } else {
            nested(block(
                "persistence_config",
                [attr("mode", Expression::String("DISABLED".to_string()))],
            ))
        };
        // A customer key encrypts the instance's data at rest. Memorystore takes it only at create,
        // so the instance waits for the service agent's grant.
        let store_key = redis
            .encryption_key
            .as_ref()
            .map(|key| {
                grant_store_key(ctx, &mut fragment, key, label, "memorystore.googleapis.com")
            })
            .transpose()?;
        let mut instance = vec![
            attr("project", expr::raw("var.gcp_project")),
            attr("location", expr::raw("var.gcp_region")),
            attr("instance_id", resource_prefix_template(&redis.id)),
            attr("mode", Expression::String("CLUSTER_DISABLED".to_string())),
            attr(
                "engine_version",
                Expression::String(ENGINE_VERSION.to_string()),
            ),
            attr("node_type", Expression::String(node_type.to_string())),
            attr("shard_count", Expression::Number(hcl::Number::from(1))),
            attr(
                "replica_count",
                Expression::Number(hcl::Number::from(i64::from(redis.high_availability))),
            ),
            // Never AUTH_DISABLED: the server generates the token and workloads read it from Memorystore.
            attr(
                "authorization_mode",
                Expression::String("TOKEN_AUTH".to_string()),
            ),
            attr(
                "transit_encryption_mode",
                Expression::String("SERVER_AUTHENTICATION".to_string()),
            ),
            attr(
                "engine_configs",
                expr::object([(
                    "maxmemory-policy",
                    Expression::String(gcp_eviction_policy(redis.eviction_policy).to_string()),
                )]),
            ),
            attr("labels", labels(ctx, "redis")),
            attr("deletion_protection_enabled", Expression::Bool(true)),
            persistence,
            nested(block(
                "desired_psc_auto_connections",
                [
                    attr("network", expr::raw(format!("local.{network_local}"))),
                    attr("project_id", expr::raw("var.gcp_project")),
                ],
            )),
        ];
        // The endpoint needs the policy first. The owner's own instance references it through
        // `depends_on` too, since nothing else ties them together.
        let mut depends_on = vec![format!(
            "google_network_connectivity_service_connection_policy.{SHARED_POLICY_LABEL}"
        )];
        if let Some(store_key) = &store_key {
            instance.push(attr("kms_key", store_key.name.clone()));
            depends_on.push(store_key.grant_dependency.clone());
        }
        instance.push(attr(
            "depends_on",
            expr::raw(format!("[{}]", depends_on.join(", "))),
        ));
        fragment.resource_blocks.push(resource_block(
            "google_memorystore_instance",
            label,
            instance,
        ));

        emit_family_iam(ctx, &mut fragment, "redis", &redis.id)?;

        // Setup owns the instance; uninstall leaves it. The policy stays with it because deleting
        // it while the instance exists is rejected.
        fragment.retain_resources(|kind, _| {
            is_store_key_grant(kind)
                || matches!(
                    kind,
                    "google_memorystore_instance"
                        | "google_network_connectivity_service_connection_policy"
                )
        });
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let redis = downcast::<Redis>(ctx, Redis::RESOURCE_TYPE)?;
        let (node_type, _) = redis.memorystore_node_type()?;
        let label = required_label(ctx)?;
        Ok(expr::object([
            ("projectId", expr::raw("var.gcp_project")),
            (
                "instanceName",
                expr::traversal(["google_memorystore_instance", label, "instance_id"]),
            ),
            ("region", expr::raw("var.gcp_region")),
            (
                "endpointIp",
                expr::raw(format!(
                    "google_memorystore_instance.{label}.endpoints[0].connections[0].psc_auto_connection[0].ip_address"
                )),
            ),
            ("port", Expression::Number(hcl::Number::from(REDIS_PORT))),
            ("nodeType", Expression::String(node_type.to_string())),
            (
                "engineVersion",
                Expression::String(ENGINE_VERSION.to_string()),
            ),
            (
                "highAvailability",
                Expression::Bool(redis.high_availability),
            ),
            ("persistence", Expression::Bool(redis.persistence)),
        ]))
    }

    fn emit_binding_ref(&self, _ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        // The binding needs the instance CA, which exists only after the import step. The
        // controller publishes it from there.
        Ok(None)
    }
}

fn gcp_eviction_policy(policy: RedisEvictionPolicy) -> &'static str {
    policy.as_str()
}

fn expr_to_hcl(expression: &Expression) -> Result<String> {
    hcl::format::to_string(expression).map_err(|error| {
        AlienError::new(ErrorData::GenericError {
            message: format!("failed to render a network expression: {error}"),
        })
    })
}
