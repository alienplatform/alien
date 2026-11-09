//! Redis permission sets, checked by rendering them for a stack with two Redis resources and
//! asking whether concrete calls are allowed. AWS uses the IAM evaluation model in
//! `iam_policy`; GCP conditions and Azure scopes are read from the rendered bindings.
mod common;
mod iam_policy;

use alien_core::{Redis, ResourceLifecycle, Stack};
use alien_permissions::{
    generators::{
        AwsIamStatement, AwsRuntimePermissionsGenerator, AzureRuntimePermissionsGenerator,
        GcpRuntimePermissionsGenerator,
    },
    get_permission_set,
    initial_setup::{generate_aws_initial_setup_policy, initial_setup_permission_set_ids},
    BindingTarget, PermissionContext,
};
use iam_policy::{evaluate, Decision, Request};

const ELASTICACHE: &str = "arn:aws:elasticache:us-east-1:123456789012";
const SECRETS: &str = "arn:aws:secretsmanager:us-east-1:123456789012:secret";
const EC2: &str = "arn:aws:ec2:us-east-1:123456789012";

const OWN: &[(&str, &str)] = &[
    ("deployment", "my-stack"),
    ("resource", "cache"),
    ("managed-by", "runtime"),
];
const OTHER_STACK: &[(&str, &str)] = &[
    ("deployment", "my-stack-prod"),
    ("resource", "cache"),
    ("managed-by", "runtime"),
];
const SIBLING: &[(&str, &str)] = &[
    ("deployment", "my-stack"),
    ("resource", "sessions"),
    ("managed-by", "runtime"),
];

fn aws(id: &str, target: BindingTarget, resource: &str) -> Vec<AwsIamStatement> {
    let context = PermissionContext::new()
        .with_stack_prefix("my-stack")
        .with_resource_name(resource)
        .with_aws_account_id("123456789012")
        .with_aws_region("us-east-1");
    AwsRuntimePermissionsGenerator::new()
        .generate_policy(get_permission_set(id).unwrap(), target, &context)
        .unwrap()
        .statement
}

fn allowed(statements: &[AwsIamStatement], request: &Request) -> bool {
    evaluate(statements, request) == Decision::Allowed
}

/// Every call the controller makes on its own objects passes, for both the stack-wide render and
/// the render bound to the `cache` resource.
#[test]
fn aws_provision_allows_the_controllers_own_calls() {
    for (name, statements) in [
        (
            "stack",
            aws("redis/provision", BindingTarget::Stack, "cache"),
        ),
        (
            "resource",
            aws("redis/provision", BindingTarget::Resource, "cache"),
        ),
    ] {
        let group = format!("{ELASTICACHE}:replicationgroup:my-stack-cache");
        let node = format!("{ELASTICACHE}:cluster:my-stack-cache-001");
        let subnets = format!("{ELASTICACHE}:subnetgroup:my-stack-cache-redis-subnets");
        let params = format!("{ELASTICACHE}:parametergroup:my-stack-cache-1a2b3c");
        let secret = format!("{SECRETS}:my-stack-cache-auth-AbCdEf");
        let sg = format!("{EC2}:security-group/sg-0123456789abcdef0");
        let calls = vec![
            Request::new("elasticache:CreateReplicationGroup", group.clone()).request_tags(OWN),
            Request::new("elasticache:CreateReplicationGroup", node.clone()).request_tags(OWN),
            Request::new("elasticache:CreateReplicationGroup", subnets.clone()).resource_tags(OWN),
            Request::new("elasticache:CreateReplicationGroup", params.clone()).resource_tags(OWN),
            Request::new("elasticache:CreateCacheSubnetGroup", subnets.clone()).request_tags(OWN),
            Request::new("elasticache:CreateCacheParameterGroup", params.clone()).request_tags(OWN),
            Request::new("elasticache:ModifyCacheParameterGroup", params.clone())
                .resource_tags(OWN),
            Request::new("elasticache:ModifyReplicationGroup", group.clone()).resource_tags(OWN),
            Request::new("elasticache:ModifyReplicationGroup", node.clone()).resource_tags(OWN),
            Request::new("elasticache:DeleteReplicationGroup", group.clone()).resource_tags(OWN),
            Request::new("elasticache:DeleteCacheParameterGroup", params.clone())
                .resource_tags(OWN),
            Request::new("elasticache:DeleteCacheSubnetGroup", subnets.clone()).resource_tags(OWN),
            Request::new("elasticache:AddTagsToResource", group.clone()).request_tags(OWN),
            Request::new("elasticache:DescribeReplicationGroups", group.clone()),
            Request::new("elasticache:DescribeCacheClusters", node.clone()),
            Request::new("elasticache:DescribeCacheParameters", params.clone()),
            Request::new("elasticache:ListTagsForResource", group.clone()),
            Request::new("secretsmanager:CreateSecret", secret.clone()).request_tags(OWN),
            Request::new("secretsmanager:PutSecretValue", secret.clone()).resource_tags(OWN),
            Request::new("secretsmanager:DeleteSecret", secret.clone()).resource_tags(OWN),
            Request::new("secretsmanager:TagResource", secret.clone())
                .resource_tags(OWN)
                .request_tags(OWN),
            Request::new("ec2:CreateSecurityGroup", sg.clone()),
            Request::new("ec2:CreateTags", sg.clone())
                .request_tags(OWN)
                .with("ec2:CreateAction", "CreateSecurityGroup"),
            Request::new("ec2:AuthorizeSecurityGroupIngress", sg.clone()).resource_tags(OWN),
            Request::new("ec2:DeleteSecurityGroup", sg.clone()).resource_tags(OWN),
        ];
        for call in calls {
            assert!(
                allowed(&statements, &call),
                "{name}: {} on {} denied",
                call.action,
                call.resource
            );
        }
    }
}

/// A stack cannot reach the Redis objects of a stack whose name it prefixes, and the `cache`
/// resource's grants stop at the sibling `sessions` resource.
#[test]
fn aws_provision_stops_at_other_stacks_and_sibling_resources() {
    let stack = aws("redis/provision", BindingTarget::Stack, "cache");
    let resource = aws("redis/provision", BindingTarget::Resource, "cache");
    let setup = generate_aws_initial_setup_policy(&common::create_test_context())
        .unwrap()
        .statement;

    let foreign_group = format!("{ELASTICACHE}:replicationgroup:my-stack-prod-cache");
    let foreign_secret = format!("{SECRETS}:my-stack-prod-cache-auth-AbCdEf");
    let foreign_sg = format!("{EC2}:security-group/sg-0fedcba9876543210");
    for (name, statements) in [
        ("stack", &stack),
        ("resource", &resource),
        ("setup", &setup),
    ] {
        let calls = [
            Request::new("elasticache:ModifyReplicationGroup", foreign_group.clone())
                .resource_tags(OTHER_STACK),
            Request::new("elasticache:DeleteReplicationGroup", foreign_group.clone())
                .resource_tags(OTHER_STACK),
            // An untagged object of another stack cannot be claimed by tagging it as ours.
            Request::new("secretsmanager:TagResource", foreign_secret.clone())
                .request_tags(&[("deployment", "my-stack")]),
            Request::new("secretsmanager:PutSecretValue", foreign_secret.clone())
                .resource_tags(OTHER_STACK),
            Request::new("secretsmanager:DeleteSecret", foreign_secret.clone())
                .resource_tags(OTHER_STACK),
            Request::new("ec2:DeleteSecurityGroup", foreign_sg.clone()).resource_tags(OTHER_STACK),
            Request::new("ec2:AuthorizeSecurityGroupIngress", foreign_sg.clone())
                .resource_tags(OTHER_STACK),
            // Creating something that carries another stack's tag.
            Request::new("secretsmanager:CreateSecret", foreign_secret.clone())
                .request_tags(OTHER_STACK),
            Request::new(
                "elasticache:CreateReplicationGroup",
                format!("{ELASTICACHE}:replicationgroup:my-stack-prod-cache"),
            )
            .request_tags(OTHER_STACK),
        ];
        for call in calls {
            assert!(
                !allowed(statements, &call),
                "{name} allows {} on {}",
                call.action,
                call.resource
            );
        }
    }

    let sibling_group = format!("{ELASTICACHE}:replicationgroup:my-stack-sessions");
    let sibling_secret = format!("{SECRETS}:my-stack-sessions-auth-AbCdEf");
    let sibling_sg = format!("{EC2}:security-group/sg-0aaaaaaaaaaaaaaaa");
    for call in [
        Request::new("elasticache:ModifyReplicationGroup", sibling_group.clone())
            .resource_tags(SIBLING),
        Request::new("elasticache:DeleteReplicationGroup", sibling_group).resource_tags(SIBLING),
        Request::new("secretsmanager:DeleteSecret", sibling_secret).resource_tags(SIBLING),
        Request::new("ec2:DeleteSecurityGroup", sibling_sg).resource_tags(SIBLING),
    ] {
        assert!(
            !allowed(&resource, &call),
            "cache's resource grant allows {} on {}",
            call.action,
            call.resource
        );
        assert!(
            allowed(&stack, &call),
            "the stack-wide grant covers sibling resources: {} on {}",
            call.action,
            call.resource
        );
    }
}

/// The management side writes the AUTH token but never reads it, and the setup identity can
/// read it only for the stack's own tagged secrets.
#[test]
fn aws_only_the_workload_and_setup_read_the_auth_token() {
    let secret = format!("{SECRETS}:my-stack-cache-auth-AbCdEf");
    let read = Request::new("secretsmanager:GetSecretValue", secret.clone()).resource_tags(OWN);
    for id in ["redis/provision", "redis/heartbeat", "redis/setup"] {
        let readable = allowed(&aws(id, BindingTarget::Stack, "cache"), &read);
        assert_eq!(readable, id == "redis/setup", "{id}");
    }
    let other = Request::new(
        "secretsmanager:GetSecretValue",
        format!("{SECRETS}:my-stack-prod-cache-auth-AbCdEf"),
    )
    .resource_tags(OTHER_STACK);
    assert!(!allowed(
        &aws("redis/setup", BindingTarget::Stack, "cache"),
        &other
    ));
    assert!(allowed(
        &aws("redis/data-access", BindingTarget::Resource, "cache"),
        &read
    ));
    // No management-side set can change the AUTH token's value except through ElastiCache's own
    // modify call, which the controller uses only to set a token it just wrote.
    for id in ["redis/heartbeat", "redis/setup"] {
        assert!(
            !allowed(
                &aws(id, BindingTarget::Stack, "cache"),
                &Request::new(
                    "elasticache:ModifyReplicationGroup",
                    format!("{ELASTICACHE}:replicationgroup:my-stack-cache")
                )
                .resource_tags(OWN)
            ),
            "{id}"
        );
    }
}

/// Setup holds `redis/provision` and `redis/setup` for a Frozen Redis, nothing for a Live one;
/// a Frozen Redis also gets `redis/management` through the management profile.
#[test]
fn setup_holds_provision_and_setup_for_a_frozen_redis_only() {
    let stack = |lifecycle| {
        Stack::new("s".to_string())
            .add(Redis::new("cache".to_string()).build(), lifecycle)
            .build()
    };
    let frozen = initial_setup_permission_set_ids(&stack(ResourceLifecycle::Frozen));
    assert!(
        frozen.contains(&"redis/provision".to_string()),
        "{frozen:?}"
    );
    assert!(frozen.contains(&"redis/setup".to_string()), "{frozen:?}");
    assert!(
        !frozen.contains(&"redis/management".to_string()),
        "{frozen:?}"
    );
    let live = initial_setup_permission_set_ids(&stack(ResourceLifecycle::Live));
    assert!(!live.iter().any(|id| id.starts_with("redis/")), "{live:?}");
}

/// A stack with two Redis resources keeps each rendered AWS policy under the IAM managed-policy
/// size limit, and the statement ids stay unique so two resources never collide.
#[test]
fn aws_policies_fit_iam_limits() {
    for id in [
        "redis/provision",
        "redis/management",
        "redis/setup",
        "redis/heartbeat",
        "redis/data-access",
    ] {
        let permission_set = get_permission_set(id).unwrap();
        if permission_set.platforms.aws.is_none() {
            continue;
        }
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let statements = aws(id, target, "cache");
            let size = serde_json::to_string(&statements).unwrap().len();
            // 6,144 characters is the managed-policy limit with whitespace removed.
            assert!(size < 6144, "{id} renders {size} characters");
            let mut sids: Vec<_> = statements.iter().map(|s| s.sid.clone()).collect();
            let count = sids.len();
            sids.sort();
            sids.dedup();
            assert_eq!(count, sids.len(), "{id} repeats a statement id");
        }
    }
}

const FORBIDDEN_AWS: &[&str] = &[
    "secretsmanager:GetSecretValue",
    "secretsmanager:BatchGetSecretValue",
];
const FORBIDDEN_GCP: &[&str] = &["secretmanager.versions.access", "secretmanager.secrets.get"];
const FORBIDDEN_GCP_ROLES: &[&str] = &[
    "roles/secretmanager.secretAccessor",
    "roles/secretmanager.admin",
];
const GCP_TOKEN_READS: &[&str] = &[
    "memorystore.tokenAuthUsers.list",
    "memorystore.authTokens.list",
];

/// The management-side sets never read the password: no secret-value read on AWS, none on GCP
/// beyond the Memorystore-generated token copy (which only `provision` and `management` carry,
/// never `heartbeat`), and no key listing on Azure.
#[test]
fn management_side_sets_never_read_the_password() {
    for id in ["redis/provision", "redis/heartbeat", "redis/management"] {
        let set = get_permission_set(id).unwrap();
        for permission in set.platforms.aws.iter().flatten() {
            for action in permission.grant.actions.iter().flatten() {
                assert!(!FORBIDDEN_AWS.contains(&action.as_str()), "{id}: {action}");
            }
        }
        for permission in set.platforms.gcp.iter().flatten() {
            let permissions = permission
                .grant
                .permissions
                .iter()
                .chain(permission.grant.residual_permissions.iter())
                .flatten();
            for name in permissions {
                assert!(!FORBIDDEN_GCP.contains(&name.as_str()), "{id}: {name}");
                if GCP_TOKEN_READS.contains(&name.as_str()) {
                    assert_ne!(id, "redis/heartbeat", "heartbeat must stay status-only");
                }
            }
            for role in permission.grant.predefined_roles.iter().flatten() {
                assert!(
                    !FORBIDDEN_GCP_ROLES.contains(&role.as_str()),
                    "{id}: {role}"
                );
            }
        }
        for permission in set.platforms.azure.iter().flatten() {
            for action in permission
                .grant
                .actions
                .iter()
                .flatten()
                .chain(permission.grant.data_actions.iter().flatten())
            {
                assert!(
                    !action.to_ascii_lowercase().contains("listkeys"),
                    "{id}: {action}"
                );
                assert!(
                    !action.to_ascii_lowercase().contains("regeneratekey"),
                    "{id}: {action}"
                );
            }
            for role in permission.grant.predefined_roles.iter().flatten() {
                let role = role.to_ascii_lowercase();
                assert!(!role.contains("key vault secrets"), "{id}: {role}");
                assert!(
                    !role.contains("contributor") && role != "owner",
                    "{id}: {role}"
                );
            }
        }
    }
    // The token copy exists in provision (live create) and management (frozen import).
    for id in ["redis/provision", "redis/management"] {
        let set = get_permission_set(id).unwrap();
        let has_token_read = set.platforms.gcp.iter().flatten().any(|permission| {
            permission
                .grant
                .permissions
                .iter()
                .flatten()
                .any(|name| name == "memorystore.authTokens.list")
        });
        assert!(has_token_read, "{id} must carry the GCP token copy");
    }
}

fn gcp_context(resource: &str) -> PermissionContext {
    PermissionContext::new()
        .with_stack_prefix("my-stack")
        .with_resource_name(resource)
        .with_project_name("my-project")
        .with_project_number("123456789012")
}

/// Evaluates the `==` and `startsWith` clauses of a rendered Secret Manager condition against a
/// secret (or secret version) resource name.
fn gcp_condition_allows(expression: &str, secret_resource_name: &str) -> bool {
    let atom = |atom: &str| -> bool {
        let atom = atom.trim().trim_start_matches('(');
        let quoted = || {
            let start = atom.find('"').expect("quoted value") + 1;
            let end = atom.rfind('"').expect("quoted value");
            &atom[start..end]
        };
        if atom.starts_with("resource.name ==") {
            return secret_resource_name == quoted();
        }
        if atom.starts_with("resource.name.startsWith(") {
            return secret_resource_name.starts_with(quoted());
        }
        panic!("unknown clause {atom}");
    };
    expression.split("&&").all(|clause| {
        let clause = clause.trim();
        let (negated, clause) = match clause.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, clause),
        };
        if clause.contains("resource.type ==") {
            return true;
        }
        clause.split("||").any(atom) != negated
    })
}

/// The Secret Manager write grant lands only on this stack's (or this resource's) secrets, and
/// never on the deployer secrets.
#[test]
fn gcp_secret_writes_stay_on_the_resources_own_secrets() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let own = "projects/123456789012/secrets/my-stack-cache-auth";
    let sibling = "projects/123456789012/secrets/my-stack-sessions-auth";
    let other_stack = "projects/123456789012/secrets/other-stack-cache-auth";
    let deployer = "projects/123456789012/secrets/my-stack-deployer-secrets-x";
    for id in ["redis/provision", "redis/management"] {
        for (target, covers_sibling) in [
            (BindingTarget::Stack, true),
            (BindingTarget::Resource, false),
        ] {
            let plan = generator
                .generate_grant_plan(
                    get_permission_set(id).unwrap(),
                    target,
                    &gcp_context("my-stack-cache"),
                )
                .unwrap_or_else(|e| panic!("{id}: {e}"));
            let conditions: Vec<_> = plan
                .bindings
                .iter()
                .filter_map(|b| b.condition.as_ref())
                .map(|c| c.expression.clone())
                .collect();
            assert_eq!(
                conditions.len(),
                1,
                "{id}: one conditioned (secret write) binding"
            );
            let expression = &conditions[0];
            assert!(gcp_condition_allows(expression, own), "{id}: {expression}");
            assert_eq!(
                gcp_condition_allows(expression, sibling),
                covers_sibling,
                "{id}"
            );
            assert!(!gcp_condition_allows(expression, other_stack), "{id}");
            assert!(!gcp_condition_allows(expression, deployer), "{id}");
        }
    }
}

/// A workload granted `redis/data-access` on `cache` reads that resource's connection secret
/// and its versions, and never the secret of a sibling whose id starts with `cache-`.
#[test]
fn gcp_data_access_does_not_reach_sibling_resources_with_a_longer_id() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let own = "projects/123456789012/secrets/my-stack-cache-auth";
    let own_version = "projects/123456789012/secrets/my-stack-cache-auth/versions/3";
    let sibling = "projects/123456789012/secrets/my-stack-cache-prod-auth";
    let sibling_version = "projects/123456789012/secrets/my-stack-cache-prod-auth/versions/1";
    for id in ["redis/data-access", "redis/provision", "redis/management"] {
        let plan = generator
            .generate_grant_plan(
                get_permission_set(id).unwrap(),
                BindingTarget::Resource,
                &gcp_context("my-stack-cache"),
            )
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        let expression = plan
            .bindings
            .iter()
            .filter_map(|b| b.condition.as_ref())
            .map(|c| c.expression.clone())
            .next()
            .unwrap_or_else(|| panic!("{id}: a conditioned secret binding"));
        assert!(gcp_condition_allows(&expression, own), "{id}: {expression}");
        assert!(gcp_condition_allows(&expression, own_version), "{id}");
        assert!(!gcp_condition_allows(&expression, sibling), "{id}");
        assert!(!gcp_condition_allows(&expression, sibling_version), "{id}");
    }
}

/// Azure: with two Redis resources, a resource-bound render touches only that resource's cluster
/// (or the resource group for the shared network and the operation status), and the group-wide
/// roles never carry cluster writes or key reads.
#[test]
fn azure_resource_bindings_name_only_their_own_cluster() {
    const RG: &str = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-app";
    let generator = AzureRuntimePermissionsGenerator::new();
    for resource in ["my-stack-cache", "my-stack-sessions"] {
        let context = PermissionContext::new()
            .with_stack_prefix("my-stack")
            .with_resource_name(resource)
            .with_subscription_id("00000000-0000-0000-0000-000000000000")
            .with_resource_group("rg-app");
        let plan = generator
            .generate_grant_plan(
                get_permission_set("redis/provision").unwrap(),
                BindingTarget::Resource,
                &context,
            )
            .unwrap();
        let cluster = format!("{RG}/providers/Microsoft.Cache/redisEnterprise/{resource}");
        let mut cluster_bound = 0;
        for binding in &plan.bindings {
            if binding.scope == cluster {
                cluster_bound += 1;
                continue;
            }
            assert_eq!(binding.scope, RG, "{resource}: unexpected scope");
            for role in &plan.custom_roles {
                if !matches!(&binding.role_definition, alien_permissions::generators::AzureRoleDefinitionRef::Custom { key } if *key == role.key)
                {
                    continue;
                }
                for action in &role.role_definition.actions {
                    assert!(
                        !action.starts_with("Microsoft.Cache/redisEnterprise/"),
                        "{resource}: group-wide role carries {action}"
                    );
                }
            }
        }
        assert!(cluster_bound > 0, "{resource}: cluster grants missing");
    }
}
