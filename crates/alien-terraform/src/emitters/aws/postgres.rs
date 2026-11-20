//! AWS Postgres — Aurora Serverless v2 cluster emitter.
//!
//! Mirrors the live-resource create flow in
//! the runtime controller's create flow so the setup
//! package (push deployment) and the controller (pull deployment) converge on
//! the same cloud resources:
//!
//!   1. A dedicated security group for the cluster, plus an ingress rule that
//!      admits 5432 from the stack workload security group alone — the database
//!      is never reachable from anywhere else.
//!   2. A DB subnet group across the private subnets.
//!   3. RDS-managed password generation and rotation, with only the secret ARN
//!      exposed to Terraform and the application binding.
//!   4. The Aurora Serverless v2 cluster (`minCapacity: 0` so it auto-pauses)
//!      and its single writer instance on `db.serverless`.
//!   5. The `postgres/*` IAM each owner actually holds — workload service
//!      accounts get `postgres/data-access` (`secretsmanager:GetSecretValue` on the
//!      password secret); management gets `postgres/management`, never the
//!      secret read.

use alien_core::{
    import::EmitContext, ComputeCluster, ErrorData, KubernetesCluster, KubernetesClusterProvider,
    Network, NetworkSettings, PermissionProfile, PermissionSetReference, Postgres,
    RemoteStackManagement, ResourceLifecycle, Result, ServiceAccount, AURORA_CLUSTER_PARAMETERS,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::BindingTarget;
use alien_core::aws_alarm_settings::{
    AURORA_WRITER_ALARMS, DB_INSTANCE_DIMENSION, TREAT_MISSING_DATA,
};
use crate::block::{attr, block, data_block, nested, resource_block};
use crate::emitters::aws::helpers::{
    aws_terraform_permission_context, downcast, emit_iam_role_policy_for_target_with_label,
    iam_policy_name_sanitize, nested_block, private_subnet_ids_expr, required_label,
    resource_prefix_template, security_group_ids_expr, tags, vpc_id_expr,
};
use crate::expr;
use crate::{TfEmitter, TfFragment};
use hcl::expr::Expression;

/// Port Aurora Postgres listens on; fixed.
const POSTGRES_PORT: i64 = 5432;
/// Conservative backup retention, matching the controller (`BACKUP_RETENTION_DAYS`).
const BACKUP_RETENTION_DAYS: i64 = 7;
/// Master username; fixed across backends and what the binding/import advertise.
const MASTER_USERNAME: &str = alien_core::POSTGRES_ADMIN_USERNAME;
/// Default database name created inside the cluster, from the single source of truth
/// `alien_core::POSTGRES_DATABASE_NAME` — shared with the controller, importer, and the other
/// emitters so a frozen import always binds to a database that exists.
const DATABASE_NAME: &str = alien_core::POSTGRES_DATABASE_NAME;

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsPostgresEmitter;

impl TfEmitter for AwsPostgresEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let postgres = downcast::<Postgres>(ctx, Postgres::RESOURCE_TYPE)?;
        postgres.validate_aurora_availability()?;
        let label = required_label(ctx)?;
        let key_arn = postgres
            .encryption_key
            .as_ref()
            .map(|key| {
                ctx.name_for(key.id())
                    .map(|label| crate::emitters::aws::key::key_arn(label))
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::GenericError {
                            message: format!(
                                "missing encryption Key '{}' for Postgres '{}'",
                                key.id(),
                                postgres.id
                            ),
                        })
                    })
            })
            .transpose()?;
        let mut fragment = TfFragment::default();
        fragment.minimum_setup_import_format_version =
            alien_core::import::MANAGED_POSTGRES_SETUP_IMPORT_FORMAT_VERSION;

        // Older installations keep their credential available while RDS enables
        // managed credentials. Forget these addresses without deleting the secret
        // or its version; the new binding depends on the updated cluster output.
        for resource_type in [
            "random_password",
            "aws_secretsmanager_secret",
            "aws_secretsmanager_secret_version",
        ] {
            fragment.moved_blocks.push(block(
                "removed",
                [
                    attr("from", expr::traversal([resource_type, label])),
                    nested_block("lifecycle", vec![attr("destroy", Expression::Bool(false))]),
                ],
            ));
        }

        // 1. Dedicated security group, egress-all. Ingress comes from a separate
        //    rule so the only path in is 5432 from the stack workload SG.
        let sg_name = format!("{}-pg-sg", postgres.id);
        fragment.resource_blocks.push(resource_block(
            "aws_security_group",
            label,
            [
                attr("name", resource_prefix_template(&sg_name)),
                attr(
                    "description",
                    Expression::String("Aurora Postgres security group".to_string()),
                ),
                attr("vpc_id", vpc_id_expr(ctx)),
                nested_block(
                    "egress",
                    vec![
                        attr("from_port", Expression::Number(hcl::Number::from(0i64))),
                        attr("to_port", Expression::Number(hcl::Number::from(0i64))),
                        attr("protocol", Expression::String("-1".to_string())),
                        attr(
                            "cidr_blocks",
                            Expression::Array(vec![Expression::String("0.0.0.0/0".to_string())]),
                        ),
                    ],
                ),
                attr("tags", tags(ctx, "postgres")),
            ],
        ));

        // Ingress on 5432 from the stack workload SG only. `security_group_ids_expr`
        // resolves to a list expression; index the first element as the source SG —
        // the stack provisions a single workload security group.
        let mut ingress_sources = Vec::new();
        let mut stack_sg_list: Option<String> = None;
        let network_settings = ctx.stack.resources().find_map(|(_id, entry)| {
            entry
                .config
                .downcast_ref::<Network>()
                .map(|network| &network.settings)
        });
        // A static default network has no workload security group to reference. A dynamic setup
        // module creates this rule only for create-new/use-existing; managed compute is admitted
        // separately in every mode below.
        if !matches!(network_settings, Some(NetworkSettings::UseDefault)) {
            let stack_sg_source = expr::raw(format!(
                "element({}, 0)",
                render_expression(&security_group_ids_expr(ctx))?
            ));
            let count = matches!(network_settings, Some(NetworkSettings::Create { .. }))
                .then_some("var.network_mode == \"use-default\" ? 0 : 1");
            ingress_sources.push((format!("{label}_ingress"), stack_sg_source, count));
            stack_sg_list = Some(render_expression(&security_group_ids_expr(ctx))?);
        }
        let eks_cluster = ctx.stack.resources().find_map(|(id, entry)| {
            let cluster = entry.config.downcast_ref::<KubernetesCluster>()?;
            (cluster.provider == KubernetesClusterProvider::Eks).then_some(id)
        });
        if let Some(id) = eks_cluster {
            let name = ctx.name_for(id).ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!("missing Terraform label for resource '{id}'"),
                })
            })?;
            // Kubernetes pools do not emit EC2 compute security groups. Admit the
            // EKS node security group instead, for both created and existing clusters.
            ingress_sources.push((
                format!("{label}_ingress_from_{name}"),
                expr::raw(format!(
                    "var.kubernetes_cluster_mode == \"create\" ? aws_eks_cluster.{name}[0].vpc_config[0].cluster_security_group_id : data.aws_eks_cluster.{name}_existing[0].vpc_config[0].cluster_security_group_id"
                )),
                None,
            ));
        }
        // Native compute's instance group is distinct from the supplied network
        // groups. Kubernetes borrows node capacity and emits no such group.
        for (id, entry) in ctx.stack.resources() {
            if eks_cluster.is_none()
                && entry.lifecycle == ResourceLifecycle::Frozen
                && entry.config.downcast_ref::<ComputeCluster>().is_some()
            {
                let name = ctx.name_for(id).ok_or_else(|| {
                    AlienError::new(ErrorData::GenericError {
                        message: format!("missing Terraform label for resource '{id}'"),
                    })
                })?;
                ingress_sources.push((
                    format!("{label}_ingress_from_{name}"),
                    expr::traversal(["aws_security_group", name, "id"]),
                    None,
                ));
            }
        }
        for (ingress_label, source, count) in ingress_sources {
            let mut attributes = vec![
                attr("type", Expression::String("ingress".to_string())),
                attr("protocol", Expression::String("tcp".to_string())),
                attr(
                    "from_port",
                    Expression::Number(hcl::Number::from(POSTGRES_PORT)),
                ),
                attr(
                    "to_port",
                    Expression::Number(hcl::Number::from(POSTGRES_PORT)),
                ),
                attr(
                    "security_group_id",
                    expr::traversal(["aws_security_group", label, "id"]),
                ),
                attr("source_security_group_id", source),
                attr(
                    "description",
                    Expression::String(
                        "Allow stack workloads to reach Postgres on 5432".to_string(),
                    ),
                ),
            ];
            if let Some(count) = count {
                attributes.insert(0, attr("count", expr::raw(count)));
            }
            // An existing network must bring the security group the database admits. Without a
            // check, `element` fails on the empty list with no hint at the missing input. A
            // precondition, not a variable validation: validations cannot read other variables
            // before Terraform 1.9, and this module runs on older versions.
            if let (Some(list), true) = (&stack_sg_list, ingress_label == format!("{label}_ingress")) {
                if list.contains("var.security_group_ids") {
                    attributes.push(nested(block(
                        "lifecycle",
                        [nested(block(
                            "precondition",
                            [
                                attr("condition", expr::raw(format!("length({list}) > 0"))),
                                attr(
                                    "error_message",
                                    Expression::String(
                                        "security_group_ids is required when network_mode is use-existing: the database admits traffic from that security group.".to_string(),
                                    ),
                                ),
                            ],
                        ))],
                    )));
                }
            }
            fragment.resource_blocks.push(resource_block(
                "aws_security_group_rule",
                &ingress_label,
                attributes,
            ));
        }

        // 2. DB subnet group across the private subnets.
        fragment.resource_blocks.push(resource_block(
            "aws_db_subnet_group",
            label,
            [
                attr(
                    "name",
                    resource_prefix_template(&format!("{}-subnets", postgres.id)),
                ),
                attr("subnet_ids", private_subnet_ids_expr(ctx)),
                attr("tags", tags(ctx, "postgres")),
            ],
        ));

        // RDS coordinates password generation and rotation with the database.
        // Neither Terraform nor the setup operator reads the credential value.
        fragment.resource_blocks.push(resource_block(
            "aws_rds_cluster_parameter_group",
            label,
            [
                attr(
                    "name_prefix",
                    resource_prefix_template(&format!("{}-tls-", postgres.id)),
                ),
                attr(
                    "family",
                    Expression::String(postgres.aurora_parameter_group_family()?),
                ),
                attr(
                    "description",
                    Expression::String("Require TLS for Postgres connections".to_string()),
                ),
            ]
            .into_iter()
            .chain(AURORA_CLUSTER_PARAMETERS.map(|parameter| {
                nested_block(
                    "parameter",
                    vec![
                        attr("name", Expression::String(parameter.name.to_string())),
                        attr("value", Expression::String(parameter.value.to_string())),
                        attr(
                            "apply_method",
                            Expression::String(parameter.apply_method.to_string()),
                        ),
                    ],
                )
            }))
            .chain([
                nested_block(
                    "lifecycle",
                    vec![attr("create_before_destroy", Expression::Bool(true))],
                ),
                attr("tags", tags(ctx, "postgres")),
            ]),
        ));

        // 4. The Aurora Serverless v2 cluster (minCapacity 0 → auto-pause).
        fragment.resource_blocks.push(resource_block(
            "aws_rds_cluster",
            label,
            [
                attr("cluster_identifier", resource_prefix_template(&postgres.id)),
                attr(
                    "db_cluster_parameter_group_name",
                    expr::traversal(["aws_rds_cluster_parameter_group", label, "name"]),
                ),
                attr(
                    "engine",
                    Expression::String("aurora-postgresql".to_string()),
                ),
                attr(
                    "engine_version",
                    Expression::String(postgres.version.clone()),
                ),
                attr(
                    "master_username",
                    Expression::String(MASTER_USERNAME.to_string()),
                ),
                attr("manage_master_user_password", true.into()),
                attr(
                    "database_name",
                    Expression::String(DATABASE_NAME.to_string()),
                ),
                attr(
                    "db_subnet_group_name",
                    expr::traversal(["aws_db_subnet_group", label, "name"]),
                ),
                attr(
                    "vpc_security_group_ids",
                    Expression::Array(vec![expr::traversal(["aws_security_group", label, "id"])]),
                ),
                nested_block(
                    "serverlessv2_scaling_configuration",
                    vec![
                        attr(
                            "min_capacity",
                            Expression::Number(
                                hcl::Number::from_f64(0.0)
                                    .unwrap_or_else(|| hcl::Number::from(0i64)),
                            ),
                        ),
                        attr("max_capacity", max_capacity_expr(postgres)?),
                    ],
                ),
                attr(
                    "backup_retention_period",
                    Expression::Number(hcl::Number::from(BACKUP_RETENTION_DAYS)),
                ),
                attr("skip_final_snapshot", Expression::Bool(true)),
                attr("storage_encrypted", Expression::Bool(true)),
                attr("kms_key_id", key_arn.clone().unwrap_or(Expression::Null)),
                attr("apply_immediately", Expression::Bool(true)),
                attr("tags", tags(ctx, "postgres")),
            ],
        ));

        // Refresh credential metadata after a planned cluster change. The
        // resource provider preserves an empty computed list during an existing
        // cluster's raw-to-managed plan; a dependent metadata read defers ARN
        // resolution until RDS has actually enabled the managed secret.
        fragment.data_blocks.push(data_block(
            "aws_rds_cluster",
            &format!("{label}_credentials"),
            [
                attr(
                    "cluster_identifier",
                    expr::traversal(["aws_rds_cluster", label, "id"]),
                ),
                attr(
                    "depends_on",
                    Expression::Array(vec![expr::traversal(["aws_rds_cluster", label])]),
                ),
            ],
        ));

        // 5. The single writer instance on db.serverless.
        fragment.resource_blocks.push(resource_block(
            "aws_rds_cluster_instance",
            label,
            [
                attr(
                    "identifier",
                    resource_prefix_template(&format!("{}-1", postgres.id)),
                ),
                attr(
                    "cluster_identifier",
                    expr::traversal(["aws_rds_cluster", label, "id"]),
                ),
                attr(
                    "instance_class",
                    Expression::String("db.serverless".to_string()),
                ),
                attr(
                    "engine",
                    Expression::String("aurora-postgresql".to_string()),
                ),
                // RDS defaults this to true for the default DB subnet group. Keep the database
                // private regardless of which network mode the customer selects.
                attr("publicly_accessible", Expression::Bool(false)),
                attr(
                    "performance_insights_enabled",
                    Expression::Bool(key_arn.is_some()),
                ),
                attr(
                    "performance_insights_kms_key_id",
                    key_arn.unwrap_or(Expression::Null),
                ),
                attr("tags", tags(ctx, "postgres")),
            ],
        ));

        // CloudWatch alarms on the writer, the same set the controller puts on a cluster it
        // creates. They are attachments of the writer: created, updated and deleted with it.
        for alarm in AURORA_WRITER_ALARMS {
            fragment.resource_blocks.push(resource_block(
                "aws_cloudwatch_metric_alarm",
                &format!("{label}_{}", alarm.name_suffix.replace('-', "_")),
                [
                    attr(
                        "alarm_name",
                        resource_prefix_template(&format!("{}-{}", postgres.id, alarm.name_suffix)),
                    ),
                    attr(
                        "alarm_description",
                        Expression::String(alarm.description.to_string()),
                    ),
                    attr("namespace", Expression::String(alarm.namespace.to_string())),
                    attr(
                        "metric_name",
                        Expression::String(alarm.metric_name.to_string()),
                    ),
                    attr(
                        "dimensions",
                        expr::object([(
                            DB_INSTANCE_DIMENSION,
                            expr::traversal(["aws_rds_cluster_instance", label, "identifier"]),
                        )]),
                    ),
                    attr("statistic", Expression::String(alarm.statistic.to_string())),
                    attr("period", Expression::from(i64::from(alarm.period_secs))),
                    attr(
                        "evaluation_periods",
                        Expression::from(i64::from(alarm.evaluation_periods)),
                    ),
                    attr("threshold", Expression::from(alarm.threshold)),
                    attr(
                        "comparison_operator",
                        Expression::String(alarm.comparison_operator.to_string()),
                    ),
                    attr(
                        "treat_missing_data",
                        Expression::String(TREAT_MISSING_DATA.to_string()),
                    ),
                    attr("tags", tags(ctx, "postgres")),
                ],
            ));
        }

        // Keep the existing manual-rotation contract. Cancelling before the
        // writer is available is ineffective: Aurora enables rotation again.
        fragment.resource_blocks.push(resource_block(
            "aws_secretsmanager_secret_rotation",
            label,
            [
                attr(
                    "secret_id",
                    expr::raw(format!(
                        "data.aws_rds_cluster.{label}_credentials.master_user_secret[0].secret_arn"
                    )),
                ),
                attr("rotation_enabled", false.into()),
                attr(
                    "depends_on",
                    Expression::Array(vec![expr::traversal(["aws_rds_cluster_instance", label])]),
                ),
            ],
        ));

        // 6. postgres/data-access IAM — read the password secret.
        emit_postgres_iam(ctx, &mut fragment, label)?;

        // Setup owns the database; uninstall leaves it, like the other clouds. Keep the cluster
        // (its RDS-managed password secret lives with it) and writer, plus the subnet group,
        // parameter group and security group AWS refuses to delete while the cluster exists.
        // Ingress rules reference the workload security group and go with the access grants.
        fragment.retain_resources(|kind, _| {
            matches!(
                kind,
                "aws_rds_cluster"
                    | "aws_rds_cluster_instance"
                    | "aws_db_subnet_group"
                    | "aws_rds_cluster_parameter_group"
                    | "aws_security_group"
            )
        });
        Ok(fragment)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        let label = required_label(ctx)?;
        Ok(expr::object([
            (
                "clusterIdentifier",
                expr::traversal(["aws_rds_cluster", label, "cluster_identifier"]),
            ),
            (
                "instanceIdentifier",
                expr::traversal(["aws_rds_cluster_instance", label, "identifier"]),
            ),
            (
                "subnetGroupName",
                expr::traversal(["aws_db_subnet_group", label, "name"]),
            ),
            (
                "securityGroupId",
                expr::traversal(["aws_security_group", label, "id"]),
            ),
            (
                "passwordSecretArn",
                expr::raw(format!(
                    "data.aws_rds_cluster.{label}_credentials.master_user_secret[0].secret_arn"
                )),
            ),
            (
                "clusterEndpoint",
                expr::traversal(["aws_rds_cluster", label, "endpoint"]),
            ),
            (
                "database",
                expr::traversal(["aws_rds_cluster", label, "database_name"]),
            ),
            ("username", Expression::String(MASTER_USERNAME.to_string())),
            ("managedPassword", true.into()),
            (
                "engineVersion",
                expr::traversal(["aws_rds_cluster", label, "engine_version"]),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
        let label = required_label(ctx)?;
        Ok(Some(expr::object([
            ("service", Expression::String("aurora-managed".to_string())),
            (
                "clusterEndpoint",
                expr::traversal(["aws_rds_cluster", label, "endpoint"]),
            ),
            ("port", Expression::Number(hcl::Number::from(POSTGRES_PORT))),
            (
                "database",
                expr::traversal(["aws_rds_cluster", label, "database_name"]),
            ),
            ("username", Expression::String(MASTER_USERNAME.to_string())),
            (
                "passwordSecretArn",
                expr::raw(format!(
                    "data.aws_rds_cluster.{label}_credentials.master_user_secret[0].secret_arn"
                )),
            ),
        ])))
    }
}

/// Render an `Expression` back to HCL text so it can be wrapped in an `element(...)` call.
/// Serializing a well-formed list expression won't fail in practice; return a structured error
/// rather than `expect`-panic if it ever does.
pub(super) fn render_expression(expression: &Expression) -> Result<String> {
    hcl::format::to_string(expression)
        .into_alien_error()
        .context(ErrorData::TemplateSerializationFailed {
            format: "Terraform HCL expression".to_string(),
            reason: "Failed to serialize the security-group list expression".to_string(),
        })
}

/// Validated Aurora capacity shared by runtime and setup.
pub fn aws_postgres_max_capacity(postgres: &Postgres) -> Result<f64> {
    postgres.aurora_max_capacity_acu()
}

/// Render the shared, validated Aurora capacity conversion.
fn max_capacity_expr(postgres: &Postgres) -> Result<Expression> {
    Ok(Expression::from(aws_postgres_max_capacity(postgres)?))
}

/// Emit the `postgres/*` IAM each owner actually holds — workloads get
/// `postgres/data-access`, management gets `postgres/management`; an owner never
/// receives a set it doesn't hold. The permission resource is the password
/// secret ARN — `postgres/data-access` grants `secretsmanager:GetSecretValue`
/// scoped to the exact RDS-managed secret ARN discovered after cluster creation.
fn emit_postgres_iam(ctx: &EmitContext<'_>, fragment: &mut TfFragment, label: &str) -> Result<()> {
    for (owner_label, permission_refs) in permission_owners(ctx, "postgres/") {
        let context = aws_terraform_permission_context()
            .with_resource_name(ctx.resource_id.to_string())
            .with_resource_id(format!(
                "${{data.aws_rds_cluster.{label}_credentials.master_user_secret[0].secret_arn}}"
            ));

        for (idx, permission_ref) in permission_refs.iter().enumerate() {
            let Some(mut permission_set) =
                permission_ref.resolve(|name| alien_permissions::get_permission_set(name).cloned())
            else {
                continue;
            };
            // Skip `/provision` (the create/update/delete ownership grant): setup emits management
            // and access permissions for the Frozen Postgres, never ownership grants. Mirrors the
            // GCP/Azure Postgres emitters and the AWS vault emitter.
            if !permission_set.id.starts_with("postgres/")
                || permission_set.id.ends_with("/provision")
            {
                continue;
            }

            // RDS chooses the secret name; the normal resource-prefix grant
            // cannot address it. Keep the declared actions/conditions and bind
            // only secret-specific entries to this cluster's exact secret ARN.
            if let Some(entries) = &mut permission_set.platforms.aws {
                for entry in entries {
                    if entry.grant.actions.as_ref().is_some_and(|actions| {
                        !actions.is_empty()
                            && actions
                                .iter()
                                .all(|action| action.starts_with("secretsmanager:"))
                    }) {
                        if let Some(binding) = &mut entry.binding.resource {
                            binding.resources = vec!["${resourceId}".to_string()];
                        }
                    }
                }
            }

            let policy_label = format!(
                "{label}_{owner_label}_{}_{idx}",
                iam_policy_name_sanitize(&permission_set.id)
            );
            let policy_name = iam_policy_name_sanitize(&format!(
                "{}-{}-{idx}",
                ctx.resource_id, permission_set.id
            ));

            emit_iam_role_policy_for_target_with_label(
                fragment,
                &owner_label,
                &permission_set,
                &policy_label,
                &policy_name,
                &context,
                BindingTarget::Resource,
            )?;
        }
    }

    Ok(())
}

/// Owners that receive `postgres/*` grants: one IAM role per workload permission
/// profile (`<profile>-sa`) plus management. All emit at `BindingTarget::Resource`,
/// matching `storage_permission_owners` / the vault emitter. Permission-set blocks
/// whose AWS action cannot be resource-scoped (e.g. `rds:DescribeDBInstances`)
/// declare a `resource` binding of `["*"]`, exactly like `vault/heartbeat`'s
/// `ssm:DescribeParameters`.
pub(super) fn permission_owners(
    ctx: &EmitContext<'_>,
    prefix: &str,
) -> Vec<(String, Vec<PermissionSetReference>)> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = permission_refs(profile, ctx.resource_id, prefix);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if let Some((label, _service_account)) = service_account_for_id(ctx, &service_account_id) {
            owners.push((label.to_string(), refs));
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = permission_refs(profile, ctx.resource_id, prefix);
        if !refs.is_empty() {
            if let Some(label) = remote_stack_management_label(ctx) {
                owners.push((label.to_string(), refs));
            }
        }
    }

    owners
}

/// Collect the `postgres/*` permission references for `resource_id` (direct refs
/// plus `*`-wildcard refs). Mirrors `storage_permission_refs`.
fn permission_refs(
    profile: &PermissionProfile,
    resource_id: &str,
    prefix: &str,
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
            .filter(|permission_ref| permission_ref.id().starts_with(prefix))
        {
            if seen_ids.insert(permission_ref.id().to_string()) {
                refs.push(permission_ref.clone());
            }
        }
    }

    refs
}

pub(super) fn service_account_for_id<'a>(
    ctx: &'a EmitContext<'_>,
    service_account_id: &str,
) -> Option<(&'a str, &'a ServiceAccount)> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    let service_account = entry.config.downcast_ref::<ServiceAccount>()?;
    let label = ctx.name_for(service_account_id)?;
    Some((label, service_account))
}

pub(super) fn remote_stack_management_label<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a str> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
            ctx.name_for(id)
        } else {
            None
        }
    })
}
