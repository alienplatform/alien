//! `release/v2`: what a release may run, workload by workload, and how an
//! enforcer builds a workload from it.
//!
//! The control plane still decides placement, replica counts within signed
//! bounds, and the values of the few environment variables the release names
//! exactly as provided. Everything that decides which code runs, how it is
//! loaded and which resources it reaches comes from the signed body or from
//! the enforcer itself: the image digest, the command, fixed environment
//! values, runtime privileges, mounts, identity, and binding values the
//! enforcer computes from its own install.

use std::collections::{BTreeMap, BTreeSet};

use alien_error::AlienError;
use serde::{Deserialize, Serialize};

use crate::envelope::Kind;
use crate::error::{ErrorData, Result};

/// Environment variables that change how a runtime loads code. They can only
/// come from the release itself: no control-plane value, config statement or
/// provided-name pattern may set them.
pub const LOADER_ENV: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "LD_BIND_NOW",
    "LD_DEBUG",
    "LD_PROFILE",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "NODE_OPTIONS",
    "NODE_PATH",
    "NODE_EXTRA_CA_CERTS",
    "NODE_TLS_REJECT_UNAUTHORIZED",
    "PYTHONPATH",
    "PYTHONHOME",
    "PYTHONSTARTUP",
    "PYTHONINSPECT",
    "PYTHONUSERBASE",
    "PYTHONWARNINGS",
    "PERL5OPT",
    "PERL5LIB",
    "PERLLIB",
    "RUBYOPT",
    "RUBYLIB",
    "JAVA_TOOL_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "_JAVA_OPTIONS",
    "CLASSPATH",
    "DOTNET_STARTUP_HOOKS",
    "CORECLR_ENABLE_PROFILING",
    "CORECLR_PROFILER",
    "CORECLR_PROFILER_PATH",
    "BASH_ENV",
    "ENV",
    "PATH",
    "GCONV_PATH",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "GIT_SSH_COMMAND",
    "PHPRC",
    "PHP_INI_SCAN_DIR",
    "BUN_OPTIONS",
    "DENO_DIR",
    "GLIBC_TUNABLES",
    "LOCPATH",
    "NLSPATH",
    "PYTHONBREAKPOINT",
    "PYTHONEXECUTABLE",
    "PYTHONPLATLIBDIR",
    "PERL5DB",
    "RUBYGEMS_GEMDEPS",
    "NODE_REPL_EXTERNAL_MODULE",
    "LUA_INIT",
    "LUA_PATH",
    "LUA_CPATH",
    "DOTNET_ADDITIONAL_DEPS",
    "DOTNET_SHARED_STORE",
    "COR_ENABLE_PROFILING",
    "COR_PROFILER",
    "COR_PROFILER_PATH",
];

/// Families of loader variables, by prefix: the dynamic linkers' and
/// exported shell functions.
pub const LOADER_ENV_PREFIXES: &[&str] = &["LD_", "DYLD_", "BASH_FUNC_", "CORECLR_"];

/// Whether the workload's runtime ships telemetry: a switch the deployment sets, which can't say
/// where the telemetry goes.
pub const RUNTIME_SEND_OTLP: &str = "ALIEN_RUNTIME_SEND_OTLP";

/// Whether `name` is a loader variable.
pub fn is_loader_env(name: &str) -> bool {
    LOADER_ENV.contains(&name)
        || LOADER_ENV_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

/// Components whose builds a release pins. An enforcer runs only a build
/// whose digest the release names. The verifier isn't one: setup pins it.
pub const COMPONENTS: &[&str] = &[
    "machine-agent/linux-x86_64",
    "machine-agent/linux-aarch64",
    "operator",
];

/// Setup templates a release can carry digests of.
pub const SETUP_TEMPLATES: &[&str] = &[
    "cloudformation",
    "terraform-aws",
    "terraform-gcp",
    "terraform-azure",
    "helm",
];

/// What one release lets run on one platform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseBody {
    /// Platform the release is for, such as `aws` or `machines`.
    pub platform: String,
    /// Release channel, such as `production`. An install accepts only the
    /// channel its setup pinned.
    pub channel: String,
    /// Every workload, by name.
    pub workloads: Vec<Workload>,
    /// What `config/v1` statements may set.
    #[serde(default)]
    pub configurable: Configurable,
    /// Digest of each enforcer build this release runs with, by name from
    /// [`COMPONENTS`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, String>,
    /// Digests of the setup templates this release renders, by name from
    /// [`SETUP_TEMPLATES`], so a customer can check what they install.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub setup: BTreeMap<String, SetupTemplate>,
    /// The full stack, for enforcers that deploy from it. It is signed with
    /// the rest of the body, so it is as trusted as the workloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<serde_json::Value>,
}

/// One setup template as the release rendered it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetupTemplate {
    /// `sha256:<hex>` of the rendered template bytes.
    pub sha256: String,
    /// Parameters (or Helm values) the customer may choose. Every other
    /// parameter must keep the template's pinned value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
}

/// Names config statements may set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configurable {
    /// Environment variable names, applied to every workload that lists them
    /// in its own `configurableEnv`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    /// Developer input ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
}

/// What kind of compute a workload is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkloadKind {
    /// A long-running container.
    Container,
    /// One container per machine.
    Daemon,
    /// A serverless function.
    Function,
    /// A sandbox runtime.
    Sandbox,
    /// A build environment.
    Build,
}

/// One workload, exactly as it may run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workload {
    /// Workload name, unique in the release.
    pub name: String,
    /// What kind of compute it is.
    pub kind: WorkloadKind,
    /// `<repository>@sha256:<hex>`.
    pub image: String,
    /// Command overriding the image's, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Fixed environment values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Exact environment names whose values the control plane provides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provided_env: Vec<String>,
    /// Resources the workload is bound to: resource id to resource type.
    /// The enforcer computes each binding's value from its own install, so
    /// the control plane can't point a binding at another resource.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, String>,
    /// Environment names a config statement may set on this workload.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configurable_env: Vec<String>,
    /// Environment names whose values come from deployer inputs, by input
    /// id. The enforcer reads the values from the customer's own store; the
    /// control plane never supplies them.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub deployer_env: BTreeMap<String, String>,
    /// Secrets the workload may receive, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<SecretBinding>,
    /// Runtime privileges and mounts.
    #[serde(default)]
    pub runtime: Runtime,
    /// Where the workload's persistent volume is mounted, if it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_mount_path: Option<String>,
    /// Permission profile the workload runs as, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// Upper bounds the control plane may scale within.
    #[serde(default)]
    pub bounds: Bounds,
    /// Function settings, for `function` workloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<FunctionSettings>,
}

/// A secret the workload may receive. The value is not signed; the name and
/// where it lands are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecretBinding {
    /// Secret name.
    pub name: String,
    /// Environment variable that receives the value, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_name: Option<String>,
    /// File the value is written to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

/// Runtime privileges. The defaults are the unprivileged ones.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Runtime {
    /// Full host capabilities.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub privileged: bool,
    /// `host` to share the host PID namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid_namespace: Option<String>,
    /// `host` to share the host network namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_mode: Option<String>,
    /// `uid[:gid]` to run as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Host bind mounts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<Mount>,
    /// Hardened execution settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<Security>,
    /// `internet` for public-internet-only egress.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub egress: Option<String>,
}

/// A host bind mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Mount {
    /// Host path.
    pub source: String,
    /// Path in the container.
    pub target: String,
    /// Mount options such as `ro`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<String>,
}

/// Hardened execution settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Security {
    /// `restricted` or `restrictedInternet`.
    pub profile: String,
    /// User id.
    pub run_as_user: i64,
    /// Group id.
    pub run_as_group: i64,
    /// Whether the root filesystem is read-only.
    pub read_only_root_filesystem: bool,
}

/// Upper bounds on what the control plane may request.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bounds {
    /// Most CPU per replica, as a quantity such as `2` or `500m`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cpu: Option<String>,
    /// Most memory per replica, as a quantity such as `512Mi`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory: Option<String>,
    /// Most replicas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_replicas: Option<u32>,
}

/// Settings of a serverless function that change what runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionSettings {
    /// Memory in MiB.
    pub memory_mb: u32,
    /// Timeout in seconds.
    pub timeout_seconds: u32,
    /// CPU architecture, such as `arm64`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub architecture: Option<String>,
}

impl ReleaseBody {
    /// Check the rules a release must follow before any of it is used.
    pub fn validate(&self) -> Result<()> {
        let mut names = BTreeSet::new();
        for workload in &self.workloads {
            if !names.insert(workload.name.as_str()) {
                return Err(body_refused(format!(
                    "workload '{}' appears twice",
                    workload.name
                )));
            }
            workload.validate()?;
            for name in &workload.configurable_env {
                if !self.configurable.env.contains(name) {
                    return Err(workload_refused(
                        &workload.name,
                        format!(
                            "configurable env '{name}' is not in the release's configurable env"
                        ),
                    ));
                }
            }
        }
        for name in &self.configurable.env {
            check_settable_name(name).map_err(body_refused)?;
        }
        if self.channel.is_empty()
            || !self
                .channel
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        {
            return Err(body_refused(format!(
                "channel '{}' is not [a-z0-9-]",
                self.channel
            )));
        }
        for (name, digest) in &self.components {
            if !COMPONENTS.contains(&name.as_str()) {
                return Err(body_refused(format!("'{name}' is not a component")));
            }
            if !is_sha256_digest(digest) {
                return Err(body_refused(format!(
                    "'{name}' digest '{digest}' is not sha256:<hex>"
                )));
            }
        }
        for (name, template) in &self.setup {
            if !SETUP_TEMPLATES.contains(&name.as_str()) {
                return Err(body_refused(format!("'{name}' is not a setup template")));
            }
            if !is_sha256_digest(&template.sha256) {
                return Err(body_refused(format!(
                    "setup '{name}' digest '{}' is not sha256:<hex>",
                    template.sha256
                )));
            }
        }
        Ok(())
    }

    /// The workload named `name`.
    pub fn workload(&self, name: &str) -> Result<&Workload> {
        self.workloads
            .iter()
            .find(|workload| workload.name == name)
            .ok_or_else(|| {
                workload_refused(name, "the signed release has no workload by this name")
            })
    }
}

impl Workload {
    fn validate(&self) -> Result<()> {
        let refuse = |reason: String| workload_refused(&self.name, reason);
        if self.name.is_empty() {
            return Err(refuse("the name is empty".to_string()));
        }
        if !is_pinned_image(&self.image) {
            return Err(refuse(format!(
                "image '{}' is not pinned by sha256 digest",
                self.image
            )));
        }
        for name in self
            .provided_env
            .iter()
            .chain(&self.configurable_env)
            .chain(self.deployer_env.keys())
        {
            check_settable_name(name).map_err(refuse)?;
            if self.env.contains_key(name) {
                return Err(refuse(format!(
                    "'{name}' has a fixed value and can't also be provided or configured"
                )));
            }
        }
        for secret in &self.secrets {
            if let Some(env_name) = &secret.env_name {
                check_settable_name(env_name).map_err(refuse)?;
            }
            if let Some(path) = &secret.file_path {
                if !path.starts_with('/') || path.split('/').any(|part| part == "..") {
                    return Err(refuse(format!(
                        "secret file path '{path}' is not absolute and plain"
                    )));
                }
            }
        }
        if let Some(max_cpu) = &self.bounds.max_cpu {
            parse_cpu_millis(max_cpu).map_err(refuse)?;
        }
        if let Some(max_memory) = &self.bounds.max_memory {
            parse_memory_bytes(max_memory).map_err(refuse)?;
        }
        for (id, resource_type) in &self.bindings {
            if id.is_empty() || resource_type.is_empty() {
                return Err(refuse("a binding needs a resource id and type".into()));
            }
            let name = crate::binding::binding_env_name(id);
            if self.env.contains_key(&name) || self.provided_env.contains(&name) {
                return Err(refuse(format!(
                    "{name} is a binding the enforcer computes; it can't also be fixed or provided"
                )));
            }
        }
        if (self.kind == WorkloadKind::Function) != self.function.is_some() {
            return Err(refuse(
                "function settings are required on functions and only on functions".into(),
            ));
        }
        Ok(())
    }
}

/// What the control plane supplies when it asks an enforcer to run one
/// replica of a workload. Nothing here can change the code that runs.
#[derive(Debug, Clone, Default)]
pub struct WorkloadRequest {
    /// The full environment the control plane computed. Signed names must
    /// carry their signed value; other names must be provided or configured.
    pub env: BTreeMap<String, String>,
    /// Secrets the control plane delivers, by name, with where they land.
    pub secrets: Vec<RequestedSecret>,
    /// CPU per replica.
    pub cpu: Option<String>,
    /// Memory per replica.
    pub memory: Option<String>,
    /// Replica count the control plane wants.
    pub replicas: Option<u32>,
}

/// Values the enforcer reads or computes itself, never from the control
/// plane.
#[derive(Debug, Clone, Default)]
pub struct LocalValues {
    /// Deployer input values, by input id, from the customer's own store.
    pub deployer_inputs: BTreeMap<String, String>,
    /// Binding values, by environment name, from [`crate::resolve_bindings`].
    pub bindings: BTreeMap<String, String>,
}

/// A secret delivery the control plane asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestedSecret {
    /// Secret name.
    pub name: String,
    /// Environment variable that receives it.
    pub env_name: Option<String>,
    /// File it is written to.
    pub file_path: Option<String>,
}

/// A workload built from its signed release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltWorkload {
    /// Workload name.
    pub name: String,
    /// The signed image.
    pub image: String,
    /// The signed command.
    pub command: Option<Vec<String>>,
    /// Signed values, then configured values, then provided values.
    pub env: BTreeMap<String, String>,
    /// The signed runtime.
    pub runtime: Runtime,
    /// The signed volume mount path.
    pub volume_mount_path: Option<String>,
    /// The signed permission profile.
    pub identity: Option<String>,
}

/// Build workload `name` from a verified release, applying config values
/// from a verified `config/v1`, the values the enforcer computed itself, and
/// the values the control plane provides.
///
/// A request that tries to change a signed or computed value, set a name the
/// release doesn't let it set, deliver an unbound secret or go past a bound
/// is refused, so the attempt is reported rather than silently dropped.
pub fn build_workload(
    release: &ReleaseBody,
    name: &str,
    config_env: &BTreeMap<String, String>,
    local: &LocalValues,
    request: &WorkloadRequest,
) -> Result<BuiltWorkload> {
    let workload = release.workload(name)?;
    let refuse = |reason: String| workload_refused(name, reason);

    let mut env = workload.env.clone();
    for (key, value) in config_env {
        if workload.configurable_env.contains(key) {
            env.insert(key.clone(), value.clone());
        }
    }
    for id in workload.bindings.keys() {
        let key = crate::binding::binding_env_name(id);
        let value = local.bindings.get(&key).ok_or_else(|| {
            refuse(format!(
                "the enforcer has no value for binding '{id}' ({key})"
            ))
        })?;
        env.insert(key, value.clone());
    }
    for (key, input) in &workload.deployer_env {
        let value = local.deployer_inputs.get(input).ok_or_else(|| {
            refuse(format!(
                "deployer input '{input}' for env {key} is not set in this deployment's store"
            ))
        })?;
        env.insert(key.clone(), value.clone());
    }
    let own_binding = crate::binding::binding_env_name(name);
    for (key, value) in &request.env {
        // Deployer inputs come from the customer's store; whatever the
        // control plane sends for them is ignored.
        if workload.deployer_env.contains_key(key) {
            continue;
        }
        if let Some(signed) = env.get(key) {
            if !same_value(signed, value) {
                return Err(refuse(format!(
                    "env {key} differs from its signed, configured or computed value"
                )));
            }
            continue;
        }
        if workload.configurable_env.contains(key) {
            return Err(refuse(format!(
                "env {key} is configurable and only a signed config statement may set it"
            )));
        }
        // The workload's own binding only describes the workload to itself.
        if *key == own_binding {
            if !crate::binding::binding_names_itself(value, name) {
                return Err(refuse(format!(
                    "env {key} is the workload's own binding but doesn't name '{name}'"
                )));
            }
            env.insert(key.clone(), value.clone());
            continue;
        }
        // Whether the runtime ships telemetry at all; where it goes is never the request's.
        if key == RUNTIME_SEND_OTLP {
            if value != "true" && value != "false" {
                return Err(refuse(format!(
                    "env {key} is a switch, true or false, not '{value}'"
                )));
            }
            env.insert(key.clone(), value.clone());
            continue;
        }
        if is_loader_env(key) || !workload.provided_env.contains(key) {
            return Err(refuse(format!("env {key} is not in the signed release")));
        }
        env.insert(key.clone(), value.clone());
    }

    for secret in &request.secrets {
        let bound = workload.secrets.iter().any(|binding| {
            binding.name == secret.name
                && binding.env_name == secret.env_name
                && binding.file_path == secret.file_path
        });
        if !bound {
            return Err(refuse(format!(
                "secret '{}' is not bound to this workload at that location",
                secret.name
            )));
        }
    }

    if let (Some(cpu), Some(max)) = (&request.cpu, &workload.bounds.max_cpu) {
        if parse_cpu_millis(cpu).map_err(refuse)? > parse_cpu_millis(max).map_err(refuse)? {
            return Err(refuse(format!("cpu {cpu} is above the signed bound {max}")));
        }
    }
    if let (Some(memory), Some(max)) = (&request.memory, &workload.bounds.max_memory) {
        if parse_memory_bytes(memory).map_err(refuse)? > parse_memory_bytes(max).map_err(refuse)? {
            return Err(refuse(format!(
                "memory {memory} is above the signed bound {max}"
            )));
        }
    }
    if let (Some(replicas), Some(max)) = (request.replicas, workload.bounds.max_replicas) {
        if replicas > max {
            return Err(refuse(format!(
                "{replicas} replicas is above the signed bound {max}"
            )));
        }
    }

    Ok(BuiltWorkload {
        name: workload.name.clone(),
        image: workload.image.clone(),
        command: workload.command.clone(),
        env,
        runtime: workload.runtime.clone(),
        volume_mount_path: workload.volume_mount_path.clone(),
        identity: workload.identity.clone(),
    })
}

/// Whether two values of one variable are the same. Binding values are JSON
/// whose key order the control plane may not keep, so JSON objects compare
/// by content.
fn same_value(expected: &str, actual: &str) -> bool {
    if expected == actual {
        return true;
    }
    match (
        serde_json::from_str::<serde_json::Value>(expected),
        serde_json::from_str::<serde_json::Value>(actual),
    ) {
        (Ok(left @ serde_json::Value::Object(_)), Ok(right)) => left == right,
        _ => false,
    }
}

/// Whether `image` is `<repository>@sha256:<64 hex>`.
pub fn is_pinned_image(image: &str) -> bool {
    image
        .rsplit_once('@')
        .is_some_and(|(repository, digest)| !repository.is_empty() && is_sha256_digest(digest))
}

/// Whether `digest` is `sha256:<64 lowercase hex>`.
pub fn is_sha256_digest(digest: &str) -> bool {
    digest.strip_prefix("sha256:").is_some_and(|hex_part| {
        hex_part.len() == 64
            && hex_part
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    })
}

fn check_settable_name(name: &str) -> std::result::Result<(), String> {
    if name.is_empty() || name.contains('=') || name.contains('\0') {
        return Err(format!("'{name}' is not a valid environment variable name"));
    }
    if is_loader_env(name) {
        return Err(format!(
            "{name} changes how code loads and can only have a fixed value"
        ));
    }
    if is_enforcer_env(name) {
        return Err(format!(
            "{name} is set by the enforcer from the signed release and can't be provided or configured"
        ));
    }
    Ok(())
}

/// Names the enforcer fills itself: the vault secrets list and binding values.
/// A control plane that could set them would pick which vault keys load or
/// where a binding points.
fn is_enforcer_env(name: &str) -> bool {
    matches!(name, "ALIEN_SECRETS" | "WORKLOAD_SECRETS")
        || name
            .strip_prefix("ALIEN_")
            .is_some_and(|rest| rest.len() > "_BINDING".len() && rest.ends_with("_BINDING"))
}

/// CPU quantity in millicores: `2`, `0.5` or `500m`.
pub fn parse_cpu_millis(quantity: &str) -> std::result::Result<u64, String> {
    let invalid = || format!("'{quantity}' is not a CPU quantity");
    if let Some(millis) = quantity.strip_suffix('m') {
        return millis.parse().map_err(|_| invalid());
    }
    let (whole, fraction) = quantity.split_once('.').unwrap_or((quantity, ""));
    if fraction.len() > 3 || (whole.is_empty() && fraction.is_empty()) {
        return Err(invalid());
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| invalid())?
    };
    let fraction: u64 = if fraction.is_empty() {
        0
    } else {
        format!("{fraction:0<3}").parse().map_err(|_| invalid())?
    };
    whole
        .checked_mul(1000)
        .and_then(|millis| millis.checked_add(fraction))
        .ok_or_else(invalid)
}

/// Memory quantity in bytes: a number with an optional `Ki`/`Mi`/`Gi`/`Ti`
/// or `K`/`M`/`G`/`T` suffix.
pub fn parse_memory_bytes(quantity: &str) -> std::result::Result<u64, String> {
    let invalid = || format!("'{quantity}' is not a memory quantity");
    const UNITS: &[(&str, u64)] = &[
        ("Ki", 1 << 10),
        ("Mi", 1 << 20),
        ("Gi", 1 << 30),
        ("Ti", 1 << 40),
        ("K", 1_000),
        ("M", 1_000_000),
        ("G", 1_000_000_000),
        ("T", 1_000_000_000_000),
    ];
    let (number, multiplier) = UNITS
        .iter()
        .find_map(|(suffix, multiplier)| {
            quantity
                .strip_suffix(suffix)
                .map(|number| (number, *multiplier))
        })
        .unwrap_or((quantity, 1));
    let number: u64 = number.parse().map_err(|_| invalid())?;
    number.checked_mul(multiplier).ok_or_else(invalid)
}

fn body_refused(reason: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::BodyRefused {
        kind: Kind::Release.as_str().to_string(),
        reason,
    })
}

fn workload_refused(workload: &str, reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::WorkloadRefused {
        workload: workload.to_string(),
        reason: reason.into(),
    })
}
