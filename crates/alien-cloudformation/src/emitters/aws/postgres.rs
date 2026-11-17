//! AWS Postgres — Aurora Serverless v2 cluster + writer instance, dedicated
//! security group, DB subnet group, generated master-password secret, and the
//! `postgres/*` IAM policies.
//!
//! The emitted shape mirrors the runtime Postgres controller's create flow
//! exactly:
//!
//!   1. A dedicated `AWS::EC2::SecurityGroup` in the workload VPC plus an
//!      `AWS::EC2::SecurityGroupIngress` admitting TCP 5432 from the stack
//!      workload security group only — never `0.0.0.0/0`.
//!   2. An `AWS::RDS::DBSubnetGroup` spanning the private subnets.
//!   3. An `AWS::SecretsManager::Secret` whose value is a generated password
//!      (hex-only, matching the controller's UUID-derived password so RDS never
//!      rejects a reserved character). The cluster reads it through a
//!      `{{resolve:secretsmanager:...}}` reference, so the raw password never
//!      lands in the template body.
//!   4. An `AWS::RDS::DBCluster` (engine `aurora-postgresql`, master user
//!      `app_admin`, db name `app`, `ServerlessV2ScalingConfiguration` min 0 / max
//!      derived from the requested memory tier, backup retention 7).
//!   5. An `AWS::RDS::DBInstance` writer on `db.serverless`.
//!   6. `postgres/*` permission sets rendered as inline `AWS::IAM::Policy`
//!      resources (notably `postgres/data-access` →
//!      `secretsmanager:GetSecretValue` on the password secret), mirroring how
//!      `storage.rs` emits IAM in CFN.

use crate::emitters::aws::helpers::{
    cf_from_json, default_network, private_subnet_ids_expr, required_logical_id, resource_config,
    security_group_ids_expr, service_account_role_id, subnet_refs, tags,
    uniquify_iam_statement_sids, vpc_id_expr, CONDITION_NETWORK_MODE_CREATE,
    PARAM_PRIVATE_SUBNET_IDS, PARAM_SECURITY_GROUP_IDS,
};
use crate::{CfEmitter, CfExpression, CfResource};
use alien_core::{
    import::EmitContext, ComputeCluster, ErrorData, KubernetesCluster, KubernetesClusterProvider,
    Network, NetworkSettings, PermissionProfile, PermissionSetReference, Postgres,
    RemoteStackManagement, ResourceLifecycle, Result, ServiceAccount, AURORA_CLUSTER_PARAMETERS,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_core::aws_alarm_settings::{
    AURORA_WRITER_ALARMS, DB_INSTANCE_DIMENSION, TREAT_MISSING_DATA,
};
use alien_permissions::{
    generators::AwsCloudFormationPermissionsGenerator, BindingTarget, PermissionContext,
};

/// Port Aurora Postgres listens on; fixed, matching the controller.
const POSTGRES_PORT: i64 = 5432;
/// Backup retention; matches `BACKUP_RETENTION_DAYS` in the controller.
const BACKUP_RETENTION_DAYS: i64 = 7;
/// Aurora Serverless v2 auto-pause floor — minCapacity 0, matching the controller.
const MIN_CAPACITY_ACU: f64 = 0.0;
/// Master username; fixed, what the binding/import advertise (matches the controller).
const MASTER_USERNAME: &str = alien_core::POSTGRES_ADMIN_USERNAME;
/// Default database created inside the cluster, from the single source of truth
/// `alien_core::POSTGRES_DATABASE_NAME`. The CFN import/binding refs advertise this exact value
/// (they don't read it back from the resource like Terraform does), so deriving it from the
/// shared const is what guarantees a frozen import records a database that exists.
const DATABASE_NAME: &str = alien_core::POSTGRES_DATABASE_NAME;
/// Condition emitted by the base CloudFormation generator for setup-owned Postgres stacks.
const CONDITION_NETWORK_MODE_HAS_NAMED_SUBNETS: &str = "NetworkModeHasNamedSubnets";

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsPostgresEmitter;

impl CfEmitter for AwsPostgresEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let postgres = resource_config::<Postgres>(ctx, Postgres::RESOURCE_TYPE)?;
        postgres.validate_aurora_availability()?;
        let logical_id = required_logical_id(ctx)?;
        let key_arn = postgres
            .encryption_key
            .as_ref()
            .map(|key| {
                ctx.name_for(key.id())
                    .map(|logical_id| CfExpression::get_att(logical_id, "Arn"))
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

        let security_group_id = format!("{logical_id}SecurityGroup");
        let ingress_id = format!("{logical_id}SecurityGroupIngress");
        let subnet_group_id = format!("{logical_id}SubnetGroup");
        let secret_id = format!("{logical_id}Secret");
        let cluster_id = format!("{logical_id}Cluster");
        let instance_id = format!("{logical_id}Instance");

        // 1. Dedicated security group in the workload VPC. Egress is left at the
        //    EC2 default (allow-all) so the cluster can reach AWS service
        //    endpoints; ingress is added by the separate ingress resource below so
        //    it can reference the stack SG without a circular property.
        let mut security_group = CfResource::new(
            security_group_id.clone(),
            "AWS::EC2::SecurityGroup".to_string(),
        );
        security_group.properties.insert(
            "GroupDescription".to_string(),
            CfExpression::from("Aurora Postgres security group"),
        );
        security_group
            .properties
            .insert("VpcId".to_string(), vpc_id_expr(ctx));
        security_group
            .properties
            .insert("Tags".to_string(), tags(ctx));

        // 2. Ingress: TCP 5432 from the stack workload security group only. The
        //    stack SG is the first (and only) entry produced by the network
        //    helper, so select index 0 of the stack security-group id list.
        let mut ingress = CfResource::new(ingress_id, "AWS::EC2::SecurityGroupIngress".to_string());
        ingress.properties.insert(
            "GroupId".to_string(),
            CfExpression::get_att(&security_group_id, "GroupId"),
        );
        ingress
            .properties
            .insert("IpProtocol".to_string(), CfExpression::from("tcp"));
        ingress
            .properties
            .insert("FromPort".to_string(), CfExpression::Integer(POSTGRES_PORT));
        ingress
            .properties
            .insert("ToPort".to_string(), CfExpression::Integer(POSTGRES_PORT));
        ingress.properties.insert(
            "SourceSecurityGroupId".to_string(),
            first_security_group_id(ctx),
        );
        ingress.properties.insert(
            "Description".to_string(),
            CfExpression::from("Allow stack workloads to reach Postgres on 5432"),
        );
        // W1035 resolves the empty optional SecurityGroupIds default even though this
        // resource is skipped for default-network, and create-new selects the created SG.
        // The use-existing input contract requires actual security-group IDs. Suppress
        // this unreachable-default warning only here, preserving all other CFN checks.
        ingress.metadata.insert(
            "cfn-lint".to_string(),
            serde_json::json!({ "config": { "ignore_checks": ["W1035"] } }),
        );
        ingress.depends_on.push(security_group_id.clone());
        // The default VPC path has no setup-owned workload security group. Managed compute gets
        // its own ingress rule below; other workloads use this rule only when setup created or was
        // given explicit network resources.
        if uses_dynamic_network_mode(ctx) {
            ingress.condition = Some(CONDITION_NETWORK_MODE_HAS_NAMED_SUBNETS.to_string());
        }

        // Managed compute attaches its own security group, not the network's group.
        // Keep the network rule for other workloads and admit each setup-owned
        // compute group on the same database port. Authentication still requires
        // the workload's explicit postgres/data-access permission.
        let mut compute_ingress = Vec::new();
        let eks_cluster = ctx.stack.resources().find_map(|(id, entry)| {
            let cluster = entry.config.downcast_ref::<KubernetesCluster>()?;
            (cluster.provider == KubernetesClusterProvider::Eks).then_some(id)
        });
        if let Some(id) = eks_cluster {
            let name = ctx.name_for(id).ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!("missing CloudFormation logical id for resource '{id}'"),
                })
            })?;
            // Logical Kubernetes pools emit no EC2 security group. EKS owns the
            // cluster node security group in every supported setup network mode.
            let mut rule = ingress.clone();
            rule.metadata.shift_remove("cfn-lint");
            rule.logical_id = format!("{logical_id}IngressFrom{name}");
            rule.condition = None;
            rule.properties.insert(
                "SourceSecurityGroupId".to_string(),
                CfExpression::get_att(format!("{name}Cluster"), "ClusterSecurityGroupId"),
            );
            compute_ingress.push(rule);
        }
        for (id, entry) in ctx.stack.resources() {
            if eks_cluster.is_some()
                || entry.lifecycle != ResourceLifecycle::Frozen
                || entry.config.downcast_ref::<ComputeCluster>().is_none()
            {
                continue;
            }
            let name = ctx.name_for(id).ok_or_else(|| {
                AlienError::new(ErrorData::GenericError {
                    message: format!("missing CloudFormation logical id for resource '{id}'"),
                })
            })?;
            let mut rule = ingress.clone();
            rule.metadata.shift_remove("cfn-lint");
            rule.logical_id = format!("{logical_id}IngressFrom{name}");
            // Managed compute has a dedicated security group in every network
            // mode, including the account default VPC. Do not inherit the
            // generic workload rule's named-network condition.
            rule.condition = None;
            rule.properties.insert(
                "SourceSecurityGroupId".to_string(),
                CfExpression::get_att(format!("{name}SecurityGroup"), "GroupId"),
            );
            compute_ingress.push(rule);
        }

        // 3. DB subnet group across the private subnets.
        let mut subnet_group = CfResource::new(
            subnet_group_id.clone(),
            "AWS::RDS::DBSubnetGroup".to_string(),
        );
        subnet_group.properties.insert(
            "DBSubnetGroupDescription".to_string(),
            CfExpression::from(format!("Subnet group for Postgres '{}'", postgres.id())),
        );
        subnet_group
            .properties
            .insert("SubnetIds".to_string(), named_private_subnet_ids_expr(ctx));
        subnet_group
            .properties
            .insert("Tags".to_string(), tags(ctx));
        // In use-default mode RDS supplies the account's default DB subnet group. Avoid asking the
        // customer for subnet IDs merely to recreate that AWS-managed default.
        if uses_dynamic_network_mode(ctx) {
            subnet_group.condition = Some(CONDITION_NETWORK_MODE_HAS_NAMED_SUBNETS.to_string());
        }

        // 4. Secrets Manager secret holding the generated master password. The
        //    cluster pulls it via `resolve:secretsmanager`, so the password is
        //    never serialized into the template — only the ARN ever leaves CFN.
        let mut secret =
            CfResource::new(secret_id.clone(), "AWS::SecretsManager::Secret".to_string());
        secret.properties.insert(
            "Name".to_string(),
            CfExpression::sub(format!("${{AWS::StackName}}-{}-credentials", postgres.id())),
        );
        secret.properties.insert(
            "Description".to_string(),
            CfExpression::from(format!(
                "Aurora Postgres master password for '{}'",
                postgres.id()
            )),
        );
        secret.properties.insert(
            "GenerateSecretString".to_string(),
            // 64 hex-only characters: same alphabet and entropy class as the
            // controller's two-UUID password, with every RDS-reserved character
            // (`/`, `@`, `"`, whitespace) excluded so the cluster never rejects it.
            CfExpression::object([
                ("PasswordLength", CfExpression::Integer(64)),
                ("ExcludePunctuation", CfExpression::from(true)),
                ("ExcludeUppercase", CfExpression::from(true)),
                (
                    "ExcludeCharacters",
                    CfExpression::from("ghijklmnopqrstuvwxyz"),
                ),
            ]),
        );
        secret.properties.insert("Tags".to_string(), tags(ctx));
        // Preserve immediate deletion of Aurora credentials on stack teardown and replacement.
        secret.deletion_policy = Some("Delete".to_string());
        secret.update_replace_policy = Some("Delete".to_string());

        let parameter_group_id = format!("{logical_id}ClusterParameterGroup");
        let mut parameter_group = CfResource::new(
            parameter_group_id.clone(),
            "AWS::RDS::DBClusterParameterGroup".to_string(),
        );
        parameter_group.properties.insert(
            "Description".to_string(),
            CfExpression::from("Require TLS for Postgres connections"),
        );
        parameter_group.properties.insert(
            "Family".to_string(),
            CfExpression::from(postgres.aurora_parameter_group_family()?),
        );
        parameter_group.properties.insert(
            "Parameters".to_string(),
            // CloudFormation has no apply method here: it changes only the values that differ,
            // applies dynamic ones immediately and leaves static ones pending a reboot.
            CfExpression::object(
                AURORA_CLUSTER_PARAMETERS
                    .map(|parameter| (parameter.name, CfExpression::from(parameter.value))),
            ),
        );
        parameter_group
            .properties
            .insert("Tags".to_string(), tags(ctx));

        // 5. The Aurora Serverless v2 cluster.
        let mut cluster = CfResource::new(cluster_id.clone(), "AWS::RDS::DBCluster".to_string());
        cluster
            .properties
            .insert("StorageEncrypted".to_string(), CfExpression::from(true));
        if let Some(key_arn) = &key_arn {
            cluster
                .properties
                .insert("KmsKeyId".to_string(), key_arn.clone());
        }
        cluster.properties.insert(
            "DBClusterParameterGroupName".to_string(),
            CfExpression::ref_(&parameter_group_id),
        );
        cluster.properties.insert(
            "Engine".to_string(),
            CfExpression::from("aurora-postgresql"),
        );
        cluster.properties.insert(
            "EngineVersion".to_string(),
            CfExpression::from(postgres.version.clone()),
        );
        cluster.properties.insert(
            "DBClusterIdentifier".to_string(),
            cluster_identifier(postgres),
        );
        cluster.properties.insert(
            "DatabaseName".to_string(),
            CfExpression::from(DATABASE_NAME),
        );
        cluster.properties.insert(
            "MasterUsername".to_string(),
            CfExpression::from(MASTER_USERNAME),
        );
        cluster.properties.insert(
            "MasterUserPassword".to_string(),
            resolve_secret_password(&secret_id),
        );
        if let Some(subnet_group_name) = cluster_subnet_group_name_expr(ctx, &subnet_group_id) {
            cluster
                .properties
                .insert("DBSubnetGroupName".to_string(), subnet_group_name);
        }
        cluster.properties.insert(
            "VpcSecurityGroupIds".to_string(),
            CfExpression::list([CfExpression::get_att(&security_group_id, "GroupId")]),
        );
        cluster.properties.insert(
            "ServerlessV2ScalingConfiguration".to_string(),
            CfExpression::object([
                ("MinCapacity", CfExpression::Number(MIN_CAPACITY_ACU)),
                (
                    "MaxCapacity",
                    CfExpression::Number(aws_postgres_max_capacity(postgres)?),
                ),
            ]),
        );
        cluster.properties.insert(
            "BackupRetentionPeriod".to_string(),
            CfExpression::Integer(BACKUP_RETENTION_DAYS),
        );
        cluster.properties.insert("Tags".to_string(), tags(ctx));
        cluster.depends_on.push(secret_id.clone());
        cluster.depends_on.push(security_group_id);
        // Aurora retains its existing destructive teardown policy in this release.
        cluster.deletion_policy = Some("Delete".to_string());
        cluster.update_replace_policy = Some("Delete".to_string());

        // 6. The single writer instance on the serverless instance class.
        let mut instance = CfResource::new(instance_id, "AWS::RDS::DBInstance".to_string());
        // Deterministic identifier (`${cluster}-1`), matching the controller's `instance_identifier`
        // and the Terraform emitter. Without it CloudFormation auto-generates a random name that
        // the `postgres/management` IAM ARN (`db:${AWS::StackName}-${id}-*`) would not match, and
        // that the frozen import would record non-deterministically.
        instance.properties.insert(
            "DBInstanceIdentifier".to_string(),
            CfExpression::sub(format!("${{AWS::StackName}}-{}-1", postgres.id())),
        );
        instance.properties.insert(
            "DBClusterIdentifier".to_string(),
            CfExpression::ref_(&cluster_id),
        );
        instance.properties.insert(
            "Engine".to_string(),
            CfExpression::from("aurora-postgresql"),
        );
        instance.properties.insert(
            "DBInstanceClass".to_string(),
            CfExpression::from("db.serverless"),
        );
        instance
            .properties
            .insert("PubliclyAccessible".to_string(), CfExpression::from(false));
        // CloudFormation defaults this to true, which reboots the writer whenever a setup
        // update changes a static cluster parameter. With false the change waits for the next
        // restart, as with Terraform and the runtime controller. Nothing this template sets on
        // the instance is deferred by it: the class never changes, and the other properties
        // apply immediately regardless. It would also defer future instance-level changes,
        // such as CACertificateIdentifier or a provisioned instance class, to the maintenance
        // window.
        instance
            .properties
            .insert("ApplyImmediately".to_string(), CfExpression::from(false));
        instance.properties.insert("Tags".to_string(), tags(ctx));
        if let Some(key_arn) = key_arn {
            instance.properties.insert(
                "EnablePerformanceInsights".to_string(),
                CfExpression::from(true),
            );
            instance
                .properties
                .insert("PerformanceInsightsKMSKeyId".to_string(), key_arn);
        }
        instance.depends_on.push(cluster_id);
        instance.deletion_policy = Some("Delete".to_string());
        instance.update_replace_policy = Some("Delete".to_string());

        // 7. CloudWatch alarms on the writer, the same set the controller puts on a cluster it
        // creates. They are attachments of the writer: created, updated and deleted with it.
        let alarms = writer_alarms(ctx, postgres, &logical_id, &instance.logical_id);

        let mut resources = vec![security_group];
        if !uses_default_network_statically(ctx) {
            resources.push(ingress);
            resources.push(subnet_group);
        }
        resources.extend([secret, parameter_group, cluster, instance]);
        resources.extend(alarms);
        resources.extend(compute_ingress);
        resources.extend(postgres_iam_policies(ctx, postgres, &secret_id)?);

        Ok(resources)
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        let postgres = resource_config::<Postgres>(ctx, Postgres::RESOURCE_TYPE)?;
        let logical_id = required_logical_id(ctx)?;
        let cluster_id = format!("{logical_id}Cluster");
        let instance_id = format!("{logical_id}Instance");
        Ok(CfExpression::object([
            ("clusterIdentifier", CfExpression::ref_(&cluster_id)),
            ("instanceIdentifier", CfExpression::ref_(&instance_id)),
            (
                "subnetGroupName",
                subnet_group_name_expr(ctx, &format!("{logical_id}SubnetGroup")),
            ),
            (
                "securityGroupId",
                CfExpression::get_att(format!("{logical_id}SecurityGroup"), "GroupId"),
            ),
            (
                "passwordSecretArn",
                CfExpression::ref_(format!("{logical_id}Secret")),
            ),
            (
                "clusterEndpoint",
                CfExpression::get_att(&cluster_id, "Endpoint.Address"),
            ),
            ("database", CfExpression::from(DATABASE_NAME)),
            ("username", CfExpression::from(MASTER_USERNAME)),
            (
                "engineVersion",
                CfExpression::from(postgres.version.clone()),
            ),
        ]))
    }

    fn emit_binding_ref(&self, ctx: &EmitContext<'_>) -> Result<Option<CfExpression>> {
        let logical_id = required_logical_id(ctx)?;
        let cluster_id = format!("{logical_id}Cluster");
        Ok(Some(CfExpression::object([
            ("service", CfExpression::from("aurora")),
            (
                "clusterEndpoint",
                CfExpression::get_att(&cluster_id, "Endpoint.Address"),
            ),
            ("port", CfExpression::Integer(POSTGRES_PORT)),
            ("database", CfExpression::from(DATABASE_NAME)),
            ("username", CfExpression::from(MASTER_USERNAME)),
            (
                "passwordSecretArn",
                CfExpression::ref_(format!("{logical_id}Secret")),
            ),
        ])))
    }
}

fn network_settings<'a>(ctx: &'a EmitContext<'_>) -> Option<&'a NetworkSettings> {
    ctx.stack.resources().find_map(|(_id, entry)| {
        entry
            .config
            .downcast_ref::<Network>()
            .map(|network| &network.settings)
    })
}

pub(super) fn uses_dynamic_network_mode(ctx: &EmitContext<'_>) -> bool {
    // Managed Kubernetes only accepts create-new/use-existing, so named subnets are
    // always present. Emit their direct references instead of unreachable default-RDS branches.
    !ctx.targets_kubernetes
        && !alien_core::restricts_network_mode(ctx.stack, false)
        && matches!(network_settings(ctx), Some(NetworkSettings::Create { .. }))
}

pub(super) fn uses_default_network_statically(ctx: &EmitContext<'_>) -> bool {
    matches!(network_settings(ctx), Some(NetworkSettings::UseDefault))
}

fn cluster_subnet_group_name_expr(
    ctx: &EmitContext<'_>,
    subnet_group_id: &str,
) -> Option<CfExpression> {
    if uses_dynamic_network_mode(ctx) {
        Some(CfExpression::if_(
            CONDITION_NETWORK_MODE_HAS_NAMED_SUBNETS,
            CfExpression::ref_(subnet_group_id),
            // RDS creates the default DB subnet group on first use only when the caller omits
            // DBSubnetGroupName. Passing the literal name "default" fails in a clean region where
            // that implicit resource has not been materialized yet.
            CfExpression::no_value(),
        ))
    } else if uses_default_network_statically(ctx) {
        None
    } else {
        Some(CfExpression::ref_(subnet_group_id))
    }
}

fn subnet_group_name_expr(ctx: &EmitContext<'_>, subnet_group_id: &str) -> CfExpression {
    if uses_dynamic_network_mode(ctx) {
        CfExpression::if_(
            CONDITION_NETWORK_MODE_HAS_NAMED_SUBNETS,
            CfExpression::ref_(subnet_group_id),
            CfExpression::from("default"),
        )
    } else if uses_default_network_statically(ctx) {
        CfExpression::from("default")
    } else {
        CfExpression::ref_(subnet_group_id)
    }
}

pub(super) fn named_private_subnet_ids_expr(ctx: &EmitContext<'_>) -> CfExpression {
    if uses_dynamic_network_mode(ctx) {
        if let Some((network_id, _network)) = default_network(ctx) {
            return CfExpression::if_(
                CONDITION_NETWORK_MODE_CREATE,
                subnet_refs(network_id, "PrivateSubnet"),
                CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS),
            );
        }
        CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS)
    } else {
        private_subnet_ids_expr(ctx)
    }
}

/// `${AWS::StackName}-{id}` — the cluster identifier the controller derives from
/// the stack prefix + resource id (`AwsPostgresController::cluster_identifier`).
/// One `AWS::CloudWatch::Alarm` per writer alarm, named `<stack>-<id>-<suffix>` as the
/// controller names them, with the stack's tags (the setup policy's alarm grant requires them).
fn writer_alarms(
    ctx: &EmitContext<'_>,
    postgres: &Postgres,
    logical_id: &str,
    instance_id: &str,
) -> Vec<CfResource> {
    AURORA_WRITER_ALARMS
        .iter()
        .map(|alarm| {
            let mut resource = CfResource::new(
                format!("{logical_id}{}Alarm", upper_camel(alarm.name_suffix)),
                "AWS::CloudWatch::Alarm".to_string(),
            );
            let properties = [
                (
                    "AlarmName",
                    CfExpression::sub(format!(
                        "${{AWS::StackName}}-{}-{}",
                        postgres.id(),
                        alarm.name_suffix
                    )),
                ),
                ("AlarmDescription", CfExpression::from(alarm.description)),
                ("Namespace", CfExpression::from(alarm.namespace)),
                ("MetricName", CfExpression::from(alarm.metric_name)),
                (
                    "Dimensions",
                    CfExpression::list([CfExpression::object([
                        ("Name", CfExpression::from(DB_INSTANCE_DIMENSION)),
                        ("Value", CfExpression::ref_(instance_id)),
                    ])]),
                ),
                ("Statistic", CfExpression::from(alarm.statistic)),
                ("Period", CfExpression::Integer(i64::from(alarm.period_secs))),
                (
                    "EvaluationPeriods",
                    CfExpression::Integer(i64::from(alarm.evaluation_periods)),
                ),
                ("Threshold", CfExpression::Number(alarm.threshold)),
                (
                    "ComparisonOperator",
                    CfExpression::from(alarm.comparison_operator),
                ),
                ("TreatMissingData", CfExpression::from(TREAT_MISSING_DATA)),
                ("Tags", tags(ctx)),
            ];
            for (key, value) in properties {
                resource.properties.insert(key.to_string(), value);
            }
            resource.depends_on.push(instance_id.to_string());
            resource
        })
        .collect()
}

/// `writer-free-memory` -> `WriterFreeMemory`, for logical ids.
fn upper_camel(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn cluster_identifier(postgres: &Postgres) -> CfExpression {
    CfExpression::sub(format!("${{AWS::StackName}}-{}", postgres.id()))
}

/// `{{resolve:secretsmanager:<arn>}}` reference to the generated master password.
/// `Ref` on a Secrets Manager secret resolves to its ARN, so we embed the ref via
/// `Fn::Sub` to build the dynamic-reference string CFN resolves at apply time.
pub(super) fn resolve_secret_password(secret_id: &str) -> CfExpression {
    CfExpression::object([(
        "Fn::Sub",
        CfExpression::list([
            CfExpression::from("{{resolve:secretsmanager:${SecretArn}}}"),
            CfExpression::object([("SecretArn", CfExpression::ref_(secret_id))]),
        ]),
    )])
}

/// The stack workload security-group id the cluster admits on 5432. The network
/// helper produces a single-element list whose first entry is the stack SG (its
/// created `GroupId`, or the `SecurityGroupIds` parameter for BYO networks), so
/// select index 0.
pub(super) fn first_security_group_id(ctx: &EmitContext<'_>) -> CfExpression {
    if uses_dynamic_network_mode(ctx) {
        if let Some((network_id, _network)) = default_network(ctx) {
            return CfExpression::if_(
                CONDITION_NETWORK_MODE_CREATE,
                CfExpression::get_att(format!("{network_id}SecurityGroup"), "GroupId"),
                select_first(CfExpression::ref_(PARAM_SECURITY_GROUP_IDS)),
            );
        }
    }
    select_first(security_group_ids_expr(ctx))
}

pub(super) fn select_first(values: CfExpression) -> CfExpression {
    CfExpression::object([(
        "Fn::Select",
        CfExpression::list([CfExpression::Integer(0), values]),
    )])
}

/// Validated Aurora capacity shared by runtime and setup.
pub fn aws_postgres_max_capacity(postgres: &Postgres) -> Result<f64> {
    postgres.aurora_max_capacity_acu()
}

/// Inline `AWS::IAM::Policy` resources for the stack's `postgres/*` permission
/// sets — chiefly `postgres/data-access`'s `secretsmanager:GetSecretValue` on this
/// resource's password secret. Mirrors `storage.rs::storage_iam_policies`: walk
/// the permission profiles (and management), resolve each `postgres/` set through
/// the AWS CloudFormation permissions generator, and attach the rendered policy
/// to the owning IAM role.
fn postgres_iam_policies(
    ctx: &EmitContext<'_>,
    postgres: &Postgres,
    secret_id: &str,
) -> Result<Vec<CfResource>> {
    let mut resources = Vec::new();
    let generator = AwsCloudFormationPermissionsGenerator::new();
    let context = postgres_permission_context(postgres);
    // Single-Postgres templates must keep their deployed policy IDs. Multiple setup-owned
    // databases need the resource name as well, independent of their declaration order.
    let multiple_databases = ctx
        .stack
        .resources()
        .filter(|(_, entry)| {
            entry.lifecycle == ResourceLifecycle::Frozen
                && entry.config.downcast_ref::<Postgres>().is_some()
        })
        .nth(1)
        .is_some();
    // A renamed inline resource must also have a different physical policy name: deleting
    // the old resource otherwise removes the replacement's attachment to the same role.
    let (policy_scope, policy_name_suffix) = if multiple_databases {
        (required_logical_id(ctx)?, "-scoped")
    } else {
        ("", "")
    };

    for (owner_index, (role_id, permission_refs)) in permission_owners(ctx, "postgres/") {
        for (permission_index, permission_ref) in permission_refs.iter().enumerate() {
            let Some(permission_set) =
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

            let policy = generator
                .generate_policy(&permission_set, BindingTarget::Resource, &context)
                .context(ErrorData::OperationNotSupported {
                    operation: "generate AWS CloudFormation Postgres IAM policy".to_string(),
                    reason: format!(
                        "failed to generate IAM policy for permission set '{}'",
                        permission_set.id
                    ),
                })?;
            if policy.statement.is_empty() {
                continue;
            }
            let policy_value = serde_json::to_value(policy).into_alien_error().context(
                ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "Failed to serialize postgres IAM policy".to_string(),
                },
            )?;
            let CfExpression::Object(mut policy_object) = cf_from_json(policy_value)? else {
                return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "policy did not serialize to a JSON object".to_string(),
                }));
            };
            let Some(CfExpression::List(policy_statements)) =
                policy_object.shift_remove("Statement")
            else {
                // `policy.statement.is_empty()` was already guarded above, so a missing or
                // non-list `Statement` is an invariant violation, not an empty policy to skip.
                return Err(AlienError::new(ErrorData::TemplateSerializationFailed {
                    format: "CloudFormation IAM policy".to_string(),
                    reason: "policy serialized without a `Statement` list".to_string(),
                }));
            };

            let policy_id =
                format!("{role_id}Postgres{policy_scope}Permission{owner_index}{permission_index}");
            let mut policy_resource = CfResource::new(policy_id, "AWS::IAM::Policy".to_string());
            policy_resource.properties.insert(
                "PolicyName".to_string(),
                CfExpression::sub(format!(
                    "${{AWS::StackName}}-{}-postgres-{owner_index}-{permission_index}{policy_name_suffix}",
                    postgres.id()
                )),
            );
            policy_resource.properties.insert(
                "PolicyDocument".to_string(),
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list(uniquify_iam_statement_sids(policy_statements)),
                    ),
                ]),
            );
            policy_resource.properties.insert(
                "Roles".to_string(),
                CfExpression::list([CfExpression::ref_(&role_id)]),
            );
            policy_resource.depends_on.push(secret_id.to_string());
            policy_resource.depends_on.push(role_id.clone());
            resources.push(policy_resource);
        }
    }

    Ok(resources)
}

/// CFN-flavored permission context for the postgres policies. `stack_prefix` is
/// empty so the generator expands `${stackPrefix}` to `${AWS::StackName}`;
/// `resource_name` is the bare resource id (matching the AWS Terraform emitter). The same context
/// is reused for every `postgres/*` set, so it MUST be the bare id: `postgres/management`'s
/// cluster/db ARN bindings resolve to `${prefix}-{id}` (not a non-existent `{id}-credentials`
/// resource), and `postgres/data-access`'s `${prefix}-${resourceName}-*` secret binding still matches
/// the `{id}-credentials` secret via the trailing wildcard.
fn postgres_permission_context(postgres: &Postgres) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("")
        .with_aws_region("${AWS::Region}")
        .with_aws_account_id("${AWS::AccountId}")
        .with_managing_role_arn("${ManagingRoleArn}")
        .with_resource_name(postgres.id().to_string())
}

/// IAM roles in the stack that hold a `postgres/*` permission ref for this
/// resource, paired with those refs. Mirrors `storage.rs::storage_permission_owners`:
/// service-account roles plus the management role.
pub(super) fn permission_owners(
    ctx: &EmitContext<'_>,
    prefix: &str,
) -> Vec<(usize, (String, Vec<PermissionSetReference>))> {
    let mut owners = Vec::new();
    for (profile_name, profile) in ctx.stack.permission_profiles() {
        let refs = permission_refs(profile, ctx.resource_id, prefix);
        if refs.is_empty() {
            continue;
        }

        let service_account_id = format!("{profile_name}-sa");
        if service_account_for_id(ctx, &service_account_id).is_some() {
            if let Some(role_id) = service_account_role_id(ctx, profile_name) {
                owners.push((role_id, refs));
            }
        }
    }

    if let Some(profile) = ctx.stack.management().profile() {
        let refs = permission_refs(profile, ctx.resource_id, prefix);
        if !refs.is_empty() {
            if let Some(role_id) = remote_stack_management_role_id(ctx) {
                owners.push((role_id, refs));
            }
        }
    }

    owners.into_iter().enumerate().collect()
}

/// `postgres/*` permission refs a profile holds for this resource, de-duplicated,
/// including `*`-wildcard refs scoped to `postgres/`.
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
) -> Option<&'a ServiceAccount> {
    let (_id, entry) = ctx
        .stack
        .resources()
        .find(|(id, _entry)| id.as_str() == service_account_id)?;
    entry.config.downcast_ref::<ServiceAccount>()
}

pub(super) fn remote_stack_management_role_id(ctx: &EmitContext<'_>) -> Option<String> {
    ctx.stack.resources().find_map(|(id, entry)| {
        if entry.config.resource_type() != RemoteStackManagement::RESOURCE_TYPE {
            return None;
        }
        let logical_id = ctx.name_for(id)?;
        if logical_id == "Management" {
            Some("ManagementRole".to_string())
        } else {
            Some(format!("{logical_id}Role"))
        }
    })
}
