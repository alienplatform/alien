//! Field lists of the older Operators' structs that deny unknown fields, copied
//! from the v3.3.31 and v3.3.62 tags of `alien-core` (every `deny_unknown_fields`
//! struct that still exists, as serde field names in snake case).
//!
//! To cover a newer release: check out its tag, list the fields of each struct
//! below from its source, and add a `V<version>` constant and a line in
//! `OLDER_OPERATORS` in `OLDER_OPERATORS` below.

const V3_3_31: &[(&str, &[&str])] = &[
    ("Ai", &["id"]),
    ("ArtifactRegistry", &["id", "replication_regions"]),
    (
        "AwsClientConfig",
        &["account_id", "credentials", "region", "service_overrides"],
    ),
    (
        "AwsImpersonationConfig",
        &[
            "duration_seconds",
            "external_id",
            "role_arn",
            "session_name",
            "target_region",
        ],
    ),
    ("AwsOpenSearch", &["capacity", "collection_type", "id"]),
    ("AwsOpenSearchCapacity", &["indexing", "search"]),
    ("AwsOpenSearchCapacityRange", &["max_ocu", "min_ocu"]),
    ("AwsRemoteBindingsImportData", &["role_arn", "role_name"]),
    ("AwsServiceOverrides", &["endpoints"]),
    (
        "AwsWebIdentityConfig",
        &[
            "duration_seconds",
            "role_arn",
            "session_name",
            "web_identity_token_file",
        ],
    ),
    (
        "AzureClientConfig",
        &[
            "credentials",
            "region",
            "service_overrides",
            "subscription_id",
            "tenant_id",
        ],
    ),
    ("AzureContainerAppsEnvironment", &["id"]),
    (
        "AzureImpersonationConfig",
        &[
            "client_id",
            "scope",
            "target_region",
            "target_subscription_id",
            "tenant_id",
        ],
    ),
    (
        "AzureRemoteBindingsImportData",
        &["client_id", "identity_id", "principal_id", "tenant_id"],
    ),
    ("AzureResourceGroup", &["id"]),
    ("AzureServiceBusNamespace", &["id"]),
    ("AzureServiceOverrides", &["endpoints"]),
    ("AzureStorageAccount", &["id"]),
    (
        "Build",
        &["compute_type", "environment", "id", "links", "permissions"],
    ),
    (
        "ComputeCluster",
        &[
            "capacity_groups",
            "container_cidr",
            "dynamic_container_pool",
            "failure_domain_spread",
            "id",
            "node_permissions",
            "node_permissions_platforms",
            "selected_failure_domains",
        ],
    ),
    (
        "Container",
        &[
            "autoscaling",
            "cluster",
            "code",
            "command",
            "commands_enabled",
            "cpu",
            "environment",
            "ephemeral_storage",
            "gpu",
            "health_check",
            "id",
            "kubernetes_liveness_probe",
            "kubernetes_readiness_probe",
            "kubernetes_secret_mounts",
            "links",
            "memory",
            "permissions",
            "persistent_storage",
            "pool",
            "ports",
            "public_endpoints",
            "replicas",
            "security",
            "stateful",
            "stop_grace_period_seconds",
            "tunnel",
        ],
    ),
    (
        "Daemon",
        &[
            "cluster",
            "code",
            "command",
            "commands_enabled",
            "cpu",
            "environment",
            "health_check",
            "id",
            "links",
            "memory",
            "permissions",
            "pool",
            "public_endpoints",
            "runtime",
            "stop_grace_period_seconds",
        ],
    ),
    (
        "DaemonRuntime",
        &[
            "mounts",
            "network_mode",
            "pid_namespace",
            "privileged",
            "user",
        ],
    ),
    ("DaemonRuntimeMount", &["options", "source", "target"]),
    ("Email", &["domains", "events", "id", "inbound"]),
    ("EmailEvents", &["queue"]),
    ("EmailInbound", &["storage"]),
    ("GcpAgentPlatformEngine", &["id"]),
    (
        "GcpClientConfig",
        &[
            "credentials",
            "project_id",
            "project_number",
            "region",
            "service_overrides",
        ],
    ),
    (
        "GcpImpersonationConfig",
        &[
            "delegates",
            "lifetime",
            "scopes",
            "service_account_email",
            "target_project_id",
            "target_region",
        ],
    ),
    (
        "GcpRemoteBindingsImportData",
        &[
            "project_id",
            "service_account_email",
            "service_account_unique_id",
        ],
    ),
    ("GcpServiceOverrides", &["endpoints"]),
    ("Key", &["id"]),
    (
        "KubernetesCloudReference",
        &[
            "account_id",
            "cluster_id",
            "cluster_name",
            "project_id",
            "region",
            "resource_group",
            "subscription_id",
        ],
    ),
    (
        "KubernetesCluster",
        &[
            "cloud",
            "heartbeat_mode",
            "id",
            "namespace",
            "ownership",
            "provider",
        ],
    ),
    ("Kv", &["id"]),
    ("LifecycleRule", &["days", "prefix"]),
    ("Network", &["id", "settings"]),
    (
        "Postgres",
        &[
            "backend",
            "cpu",
            "high_availability",
            "id",
            "memory",
            "storage",
            "version",
        ],
    ),
    ("Queue", &["id"]),
    (
        "RemoteBindingGrant",
        &["permission_set", "resource_id", "revision"],
    ),
    ("RemoteBindings", &["grants", "id"]),
    ("RemoteStackManagement", &["id"]),
    (
        "Sandbox",
        &[
            "code",
            "egress",
            "id",
            "lifecycle",
            "limits",
            "preview_ports",
            "private_base_image",
            "privileged_supervisor",
        ],
    ),
    (
        "SandboxCapabilities",
        &[
            "domain_egress_rules",
            "egress_deny",
            "enforced_limits",
            "files",
            "jobs",
            "pause_resume",
            "preview",
            "process_limit",
            "reconnect",
            "sandbox_lifetime",
            "snapshot",
            "supervisor_isolation",
            "supervisor_pid_namespace",
        ],
    ),
    (
        "SandboxImageCommand",
        &["command", "env", "working_directory"],
    ),
    (
        "SandboxLifecyclePolicy",
        &["idle_pause_seconds", "max_lifetime_seconds"],
    ),
    ("SandboxLimits", &["cpu", "disk", "max_processes", "memory"]),
    ("SandboxPrivilegedSupervisor", &["command_uid"]),
    (
        "ServiceAccount",
        &["id", "resource_permission_sets", "stack_permission_sets"],
    ),
    ("ServiceActivation", &["id", "service_name"]),
    (
        "Storage",
        &[
            "cors_allowed_origins",
            "encryption_key",
            "id",
            "lifecycle_rules",
            "public_read",
            "versioning",
        ],
    ),
    ("Vault", &["id"]),
    (
        "Worker",
        &[
            "code",
            "commands_enabled",
            "concurrency_limit",
            "environment",
            "id",
            "links",
            "memory_mb",
            "permissions",
            "public_endpoints",
            "readiness_probe",
            "timeout_seconds",
            "triggers",
        ],
    ),
];

const V3_3_62: &[(&str, &[&str])] = &[
    ("Ai", &["id", "models"]),
    ("AiModel", &["id", "required"]),
    ("ArtifactRegistry", &["id", "replication_regions"]),
    ("AuthoredAutoscaling", &["max", "min"]),
    (
        "AuthoredEnvMapping",
        &["name", "target_resources", "var_type"],
    ),
    (
        "AuthoredInput",
        &[
            "default",
            "description",
            "env",
            "format",
            "generate",
            "kind",
            "label",
            "max",
            "max_items",
            "max_length",
            "min",
            "min_items",
            "min_length",
            "pattern",
            "placeholder",
            "platforms",
            "provided_by",
            "required",
            "values",
            "when",
        ],
    ),
    (
        "AuthoredPool",
        &[
            "architecture",
            "autoscaling",
            "cpu",
            "data_disk",
            "ephemeral_storage",
            "failure_domain_spread",
            "gpu",
            "machines",
            "memory",
            "nested_virtualization",
        ],
    ),
    (
        "AwsClientConfig",
        &["account_id", "credentials", "region", "service_overrides"],
    ),
    (
        "AwsImpersonationConfig",
        &[
            "duration_seconds",
            "external_id",
            "role_arn",
            "session_name",
            "target_region",
        ],
    ),
    ("AwsOpenSearch", &["capacity", "collection_type", "id"]),
    ("AwsOpenSearchCapacity", &["indexing", "search"]),
    ("AwsOpenSearchCapacityRange", &["max_ocu", "min_ocu"]),
    ("AwsRemoteBindingsImportData", &["role_arn", "role_name"]),
    ("AwsServiceOverrides", &["endpoints"]),
    (
        "AwsWebIdentityConfig",
        &[
            "duration_seconds",
            "role_arn",
            "session_name",
            "web_identity_token_file",
        ],
    ),
    (
        "AzureClientConfig",
        &[
            "credentials",
            "region",
            "service_overrides",
            "subscription_id",
            "tenant_id",
        ],
    ),
    ("AzureContainerAppsEnvironment", &["id"]),
    (
        "AzureImpersonationConfig",
        &[
            "client_id",
            "scope",
            "target_region",
            "target_subscription_id",
            "tenant_id",
        ],
    ),
    (
        "AzureRemoteBindingsImportData",
        &["client_id", "identity_id", "principal_id", "tenant_id"],
    ),
    ("AzureResourceGroup", &["id"]),
    ("AzureServiceBusNamespace", &["id"]),
    ("AzureServiceOverrides", &["endpoints"]),
    ("AzureStorageAccount", &["id"]),
    (
        "Build",
        &["compute_type", "environment", "id", "links", "permissions"],
    ),
    (
        "ComputeCluster",
        &[
            "capacity_groups",
            "container_cidr",
            "dynamic_container_pool",
            "failure_domain_spread",
            "id",
            "node_permissions",
            "node_permissions_platforms",
            "selected_failure_domains",
        ],
    ),
    (
        "Container",
        &[
            "autoscaling",
            "cluster",
            "code",
            "command",
            "commands_enabled",
            "cpu",
            "egress",
            "environment",
            "ephemeral_storage",
            "gpu",
            "health_check",
            "id",
            "kubernetes_liveness_probe",
            "kubernetes_readiness_probe",
            "kubernetes_secret_mounts",
            "links",
            "memory",
            "permissions",
            "persistent_storage",
            "pool",
            "ports",
            "priority",
            "public_endpoints",
            "replicas",
            "resource_choices",
            "security",
            "stateful",
            "stop_grace_period_seconds",
            "tunnel",
        ],
    ),
    ("ContainerResourceChoices", &["cpu", "memory"]),
    (
        "ContainerResourceSelection",
        &["cpu", "memory", "volume_size_gib"],
    ),
    (
        "Daemon",
        &[
            "cluster",
            "code",
            "command",
            "commands_enabled",
            "cpu",
            "environment",
            "health_check",
            "id",
            "links",
            "memory",
            "permissions",
            "pool",
            "priority",
            "public_endpoints",
            "runtime",
            "stop_grace_period_seconds",
        ],
    ),
    (
        "DaemonRuntime",
        &[
            "mounts",
            "network_mode",
            "pid_namespace",
            "privileged",
            "user",
        ],
    ),
    ("DaemonRuntimeMount", &["options", "source", "target"]),
    ("Email", &["domains", "events", "id", "inbound"]),
    ("EmailEvents", &["queue"]),
    ("EmailInbound", &["storage"]),
    ("GcpAgentPlatformEngine", &["id"]),
    (
        "GcpClientConfig",
        &[
            "credentials",
            "project_id",
            "project_number",
            "region",
            "service_overrides",
        ],
    ),
    (
        "GcpFederationAccess",
        &[
            "assertion_audience",
            "pool_id",
            "project_number",
            "provider_id",
            "service_account_email",
            "trust",
        ],
    ),
    ("GcpFederationScope", &["deployment_id"]),
    ("GcpFederationTrust", &["issuer", "subject"]),
    (
        "GcpImpersonationConfig",
        &[
            "delegates",
            "lifetime",
            "scopes",
            "service_account_email",
            "target_project_id",
            "target_region",
        ],
    ),
    (
        "GcpRemoteBindingsImportData",
        &[
            "federation",
            "project_id",
            "service_account_email",
            "service_account_unique_id",
        ],
    ),
    (
        "GcpRemoteStackManagementImportData",
        &[
            "federation",
            "management_permissions_applied",
            "project_id",
            "project_number",
            "service_account_email",
            "service_account_unique_id",
        ],
    ),
    ("GcpServiceOverrides", &["endpoints"]),
    ("Key", &["id"]),
    (
        "KubernetesCloudReference",
        &[
            "account_id",
            "cluster_id",
            "cluster_name",
            "project_id",
            "region",
            "resource_group",
            "subscription_id",
        ],
    ),
    (
        "KubernetesCluster",
        &[
            "cloud",
            "heartbeat_mode",
            "id",
            "namespace",
            "ownership",
            "provider",
        ],
    ),
    ("Kv", &["id"]),
    ("LifecycleRule", &["days", "prefix"]),
    ("ManagedCustomCertificateConfig", &[]),
    ("Network", &["id", "settings"]),
    (
        "Postgres",
        &[
            "backend",
            "cpu",
            "encryption_key",
            "high_availability",
            "id",
            "memory",
            "storage",
            "version",
        ],
    ),
    ("Queue", &["id"]),
    ("QueueBatchConfig", &["max_messages", "max_window_seconds"]),
    (
        "Redis",
        &[
            "encryption_key",
            "eviction_policy",
            "high_availability",
            "id",
            "memory",
            "persistence",
        ],
    ),
    (
        "RemoteBindingGrant",
        &["permission_set", "resource_id", "revision"],
    ),
    ("RemoteBindings", &["grants", "id"]),
    ("RemoteStackManagement", &["id"]),
    ("ResourceChoiceRange", &["default", "max", "min"]),
    (
        "Sandbox",
        &[
            "code",
            "default_policy",
            "egress",
            "id",
            "image_user",
            "lifecycle",
            "limits",
            "links",
            "preview_ports",
            "private_base_image",
            "privileged_supervisor",
        ],
    ),
    (
        "SandboxCapabilities",
        &[
            "domain_egress_rules",
            "egress_deny",
            "enforced_limits",
            "files",
            "interactive_commands",
            "jobs",
            "pause_resume",
            "preview",
            "process_limit",
            "reconnect",
            "sandbox_lifetime",
            "snapshot",
            "supervisor_isolation",
            "supervisor_pid_namespace",
        ],
    ),
    (
        "SandboxEgressGateway",
        &[
            "admin_ca_pem",
            "audience",
            "container",
            "kid",
            "replicas",
            "signing_key",
        ],
    ),
    ("SandboxEgressReplicas", &["count", "dns", "port"]),
    ("SandboxEgressSigningKey", &["key", "vault"]),
    (
        "SandboxImageCommand",
        &["command", "env", "user", "working_directory"],
    ),
    (
        "SandboxLifecyclePolicy",
        &["idle_pause_seconds", "max_lifetime_seconds"],
    ),
    ("SandboxLimits", &["cpu", "disk", "max_processes", "memory"]),
    ("SandboxPrivilegedSupervisor", &["command_uid"]),
    (
        "ServiceAccount",
        &["id", "resource_permission_sets", "stack_permission_sets"],
    ),
    ("ServiceActivation", &["id", "service_name"]),
    (
        "Storage",
        &[
            "cors_allowed_origins",
            "cors_upload_origins",
            "encryption_key",
            "id",
            "lifecycle_rules",
            "notifications",
            "public_read",
            "versioning",
        ],
    ),
    (
        "StorageNotification",
        &["events", "prefix", "queue", "suffix"],
    ),
    ("Vault", &["id"]),
    (
        "Worker",
        &[
            "code",
            "commands_enabled",
            "concurrency_limit",
            "environment",
            "id",
            "links",
            "memory_mb",
            "permissions",
            "public_endpoints",
            "readiness_probe",
            "timeout_seconds",
            "triggers",
        ],
    ),
];

use super::pull_compat::{self, Feature};

/// Older Operators the manager still serves.
const OLDER_OPERATORS: &[(&str, &[(&str, &[&str])])] = &[("3.3.31", V3_3_31), ("3.3.62", V3_3_62)];

/// How every field added to a struct that older Operators parse strictly is
/// handled: a [`Feature`] that strips or refuses it for Operators that cannot
/// read it, or the reason it never reaches an Operator's pull target.
enum Handling {
    Feature(&'static Feature),
    /// Written by the Operator or setup, never part of a pull target.
    NotInTarget(&'static str),
}

use Handling::{Feature as F, NotInTarget as Out};

fn handled() -> Vec<(&'static str, &'static str, Handling)> {
    use pull_compat::*;
    vec![
        (
            "Container",
            "commands_capability_file",
            F(&COMMANDS_CAPABILITY_FILE),
        ),
        ("Container", "egress", F(&EGRESS)),
        ("Container", "priority", F(&PRIORITY)),
        ("Container", "resource_choices", F(&RESOURCE_CHOICES)),
        (
            "Daemon",
            "commands_capability_file",
            F(&COMMANDS_CAPABILITY_FILE),
        ),
        ("Daemon", "priority", F(&PRIORITY)),
        ("Daemon", "security", F(&DAEMON_SECURITY)),
        (
            "Worker",
            "runtime_identity_exchange",
            F(&RUNTIME_IDENTITY_EXCHANGE),
        ),
        ("Worker", "security", F(&WORKER_SECURITY)),
        ("Ai", "models", F(&AI_MODELS)),
        (
            "ArtifactRegistry",
            "replication_regions_input",
            F(&REPLICATION_REGIONS_INPUT),
        ),
        ("Postgres", "encryption_key", F(&DATABASE_ENCRYPTION_KEY)),
        ("Sandbox", "default_policy", F(&SANDBOX_POLICY)),
        ("Sandbox", "links", F(&SANDBOX_POLICY)),
        ("Sandbox", "image_user", F(&SANDBOX_IMAGE_USER)),
        ("Storage", "cors_upload_origins", F(&STORAGE_UPLOAD_ORIGINS)),
        ("Storage", "notifications", F(&STORAGE_NOTIFICATIONS)),
        (
            "GcpRemoteBindingsImportData",
            "federation",
            Out("import data the setup writes into the stack state, not part of a release target"),
        ),
        (
            "SandboxEgressReplicas",
            "addresses",
            F(&AZURE_SANDBOX_INTERNET),
        ),
        (
            "SandboxEgressReplicas",
            "min_protocol",
            F(&AZURE_SANDBOX_INTERNET),
        ),
        (
            "SandboxCapabilities",
            "interactive_commands",
            Out("a capability the sandbox backend reports at runtime, never authored in a release"),
        ),
        (
            "SandboxImageCommand",
            "user",
            Out("sandbox image build output, not an authoring option or part of a release target"),
        ),
    ]
}

/// A `deny_unknown_fields` struct in `alien-core` source and its field names.
fn strict_structs(source: &str) -> Vec<(String, Vec<String>)> {
    let mut found = Vec::new();
    let mut attributes = String::new();
    let mut lines = source.lines();
    while let Some(line) = lines.next() {
        if line.starts_with("#[") || line.starts_with("///") {
            attributes.push_str(line);
            continue;
        }
        let name = line
            .strip_prefix("pub struct ")
            .and_then(|rest| rest.strip_suffix(" {"));
        if let Some(name) = name.filter(|_| attributes.contains("deny_unknown_fields")) {
            let mut fields = Vec::new();
            for line in lines.by_ref() {
                if line == "}" {
                    break;
                }
                if let Some(field) = line.strip_prefix("    pub ") {
                    if let Some((field, _)) = field.split_once(':') {
                        fields.push(field.trim().to_string());
                    }
                }
            }
            found.push((name.to_string(), fields));
        }
        attributes.clear();
    }
    found
}

fn core_sources() -> Vec<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("alien-core source") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(std::fs::read_to_string(&path).expect("source"));
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../alien-core/src");
    let mut out = Vec::new();
    walk(&root, &mut out);
    out
}

/// A field added to a struct that older Operators parse strictly makes them
/// reject the whole target. This fails until the new field is listed in
/// `handled` with the feature that strips or refuses it (see `pull_compat`),
/// or with the reason it never reaches a pull target.
#[test]
fn every_field_added_since_an_older_operator_is_handled() {
    let head: Vec<(String, Vec<String>)> = core_sources()
        .iter()
        .flat_map(|source| strict_structs(source))
        .collect();
    let handled = handled();
    let mut unhandled = Vec::new();
    let mut used = std::collections::BTreeSet::new();
    for (version, shapes) in OLDER_OPERATORS {
        for (name, fields) in &head {
            let Some((_, old)) = shapes.iter().find(|(old_name, _)| old_name == name) else {
                continue;
            };
            for field in fields.iter().filter(|field| !old.contains(&field.as_str())) {
                match handled.iter().position(|(n, f, _)| n == name && f == field) {
                    Some(index) => {
                        used.insert(index);
                    }
                    None => unhandled.push(format!("{name}.{field} (new since {version})")),
                }
            }
        }
    }
    unhandled.sort();
    unhandled.dedup();
    assert!(
        unhandled.is_empty(),
        "fields older Operators reject, with no handling in pull_compat:\n{}",
        unhandled.join("\n")
    );
    for (index, (name, field, handling)) in handled.iter().enumerate() {
        assert!(
            used.contains(&index),
            "{name}.{field} is listed as handled but is not new for any older Operator"
        );
        match handling {
            Handling::Feature(feature) => assert!(pull_compat::FEATURES.contains(&feature)),
            Handling::NotInTarget(reason) => assert!(!reason.is_empty()),
        }
    }
}
