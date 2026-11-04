//! `release/v2`: what a release may run, workload by workload, and how an
//! enforcer builds a workload from it.
//!
//! The control plane still decides placement, replica counts within signed
//! bounds, and the values of the environment variables the release lets it
//! provide (binding targets, ports, telemetry endpoints). Everything that
//! decides which code runs and how it is loaded comes from the signed body:
//! the image digest, the command, fixed environment values, runtime
//! privileges, mounts and identity.

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
];

/// Whether `name` is a loader variable.
pub fn is_loader_env(name: &str) -> bool {
    LOADER_ENV.contains(&name)
}

/// What one release lets run on one platform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseBody {
    /// Platform the release is for, such as `aws` or `machines`.
    pub platform: String,
    /// Every workload, by name.
    pub workloads: Vec<Workload>,
    /// What `config/v1` statements may set.
    #[serde(default)]
    pub configurable: Configurable,
    /// Component name to digest, for the components this release expects.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub components: BTreeMap<String, String>,
    /// Data the control plane may lease, by resource name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote_access: Vec<String>,
    /// Bounds for dynamic containers, if the release allows any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<DynamicPolicy>,
    /// Plugin name to bundle digest.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugins: BTreeMap<String, String>,
    /// Commands whose payloads need a `command/v1`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signed_commands: Vec<String>,
    /// The full stack, for enforcers that deploy from it. It is signed with
    /// the rest of the body, so it is as trusted as the workloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<serde_json::Value>,
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

/// Bounds on dynamic container specs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DynamicPolicy {
    /// Image repositories (without tag or digest) a spec may use.
    pub repositories: Vec<String>,
    /// Environment variable names a spec may set.
    #[serde(default)]
    pub env: Vec<String>,
    /// Upper bounds for each dynamic container.
    #[serde(default)]
    pub bounds: Bounds,
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
    /// Environment names whose values the control plane provides.
    #[serde(default)]
    pub provided_env: ProvidedEnv,
    /// Environment names a config statement may set on this workload.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub configurable_env: Vec<String>,
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

/// Names the control plane may provide values for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProvidedEnv {
    /// Exact names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// Name prefixes, such as the platform's binding variables.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prefixes: Vec<String>,
}

impl ProvidedEnv {
    /// Whether the control plane may provide `name`.
    pub fn allows(&self, name: &str) -> bool {
        !is_loader_env(name)
            && (self.names.iter().any(|allowed| allowed == name)
                || self
                    .prefixes
                    .iter()
                    .any(|prefix| name.starts_with(prefix.as_str())))
    }
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
        if let Some(dynamic) = &self.dynamic {
            for name in &dynamic.env {
                check_settable_name(name).map_err(body_refused)?;
            }
            for repository in &dynamic.repositories {
                if repository.is_empty() || repository.contains('@') || repository.ends_with('/') {
                    return Err(body_refused(format!(
                        "dynamic repository '{repository}' must be a repository without digest"
                    )));
                }
            }
        }
        for (name, digest) in self.components.iter().chain(&self.plugins) {
            if !is_sha256_digest(digest) {
                return Err(body_refused(format!(
                    "'{name}' digest '{digest}' is not sha256:<hex>"
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
        for prefix in &self.provided_env.prefixes {
            if prefix.is_empty() {
                return Err(refuse(
                    "an empty provided-env prefix would allow every name".into(),
                ));
            }
            if let Some(loader) = LOADER_ENV
                .iter()
                .find(|loader| loader.starts_with(prefix.as_str()))
            {
                return Err(refuse(format!(
                    "provided-env prefix '{prefix}' covers the loader variable {loader}"
                )));
            }
        }
        for name in self.provided_env.names.iter().chain(&self.configurable_env) {
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
/// from a verified `config/v1` and the values the control plane provides.
///
/// A request that tries to change a signed value, set a name the release
/// doesn't let it set, deliver an unbound secret or go past a bound is
/// refused, so the attempt is reported rather than silently dropped.
pub fn build_workload(
    release: &ReleaseBody,
    name: &str,
    config_env: &BTreeMap<String, String>,
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
    for (key, value) in &request.env {
        if let Some(signed) = env.get(key) {
            if signed != value {
                return Err(refuse(format!(
                    "env {key} differs from its signed or configured value"
                )));
            }
            continue;
        }
        if workload.configurable_env.contains(key) {
            return Err(refuse(format!(
                "env {key} is configurable and only a signed config statement may set it"
            )));
        }
        if !workload.provided_env.allows(key) {
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
    Ok(())
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
