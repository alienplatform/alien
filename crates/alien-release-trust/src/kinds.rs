//! Bodies of the statement kinds other than releases and signer lists, and
//! the checks each one needs beyond the envelope's.

use std::collections::BTreeMap;

use alien_error::AlienError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::envelope::Kind;
use crate::error::{ErrorData, Result};
use crate::release::{
    is_loader_env, is_pinned_image, is_sha256_digest, parse_cpu_millis, parse_memory_bytes,
    BuiltWorkload, ReleaseBody, Runtime,
};

/// Longest exec or debug session a statement may allow.
pub const MAX_SESSION_SECONDS: u32 = 30 * 60;

/// `config/v1`: developer config for one deployment.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigBody {
    /// Environment values, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Developer input values, by id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, serde_json::Value>,
}

impl ConfigBody {
    /// Refuse any name the release doesn't make configurable.
    pub fn check(&self, release: &ReleaseBody) -> Result<()> {
        for name in self.env.keys() {
            if is_loader_env(name) || !release.configurable.env.contains(name) {
                return Err(refused(
                    Kind::Config,
                    format!("env {name} is not configurable in the signed release"),
                ));
            }
        }
        for id in self.inputs.keys() {
            if !release.configurable.inputs.contains(id) {
                return Err(refused(
                    Kind::Config,
                    format!("input '{id}' is not configurable in the signed release"),
                ));
            }
        }
        Ok(())
    }
}

/// What a session runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionTarget {
    /// Machine id, for machine sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
    /// Workload name, for replica sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload: Option<String>,
    /// Tool, for debug sessions through the operator or gate (`kubectl`,
    /// `aws`, ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// Namespace or scope the tool may reach.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

/// `session/v1`: one exec or debug session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBody {
    /// Unique session id; an enforcer refuses one it has seen.
    pub session_id: String,
    /// What the session reaches.
    pub target: SessionTarget,
    /// Program and arguments, or absent for the default shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv: Option<Vec<String>>,
    /// Whether a terminal is allocated.
    pub tty: bool,
    /// Longest the session may last.
    pub max_duration_seconds: u32,
    /// Who asked, for the transcript.
    pub requester: String,
    /// Why, for the transcript.
    pub reason: String,
    /// The enforcer's fresh nonce, base64.
    pub verifier_nonce: String,
    /// The enforcer's ephemeral public key for this session, base64.
    pub verifier_key: String,
    /// The client's ephemeral public key, base64.
    pub client_key: String,
}

/// What the enforcer handed out for one session request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionChallenge {
    /// Nonce the enforcer generated, base64.
    pub nonce: String,
    /// Enforcer's ephemeral public key, base64.
    pub verifier_key: String,
    /// When the nonce was issued.
    pub issued_at: DateTime<Utc>,
    /// How long a nonce stays usable.
    pub nonce_lifetime_seconds: u32,
}

impl SessionBody {
    /// Check the session against the challenge the enforcer issued. The
    /// caller also records the session id and refuses one it has seen.
    pub fn check(&self, challenge: &SessionChallenge, now: DateTime<Utc>) -> Result<()> {
        if self.verifier_nonce != challenge.nonce {
            return Err(refused(
                Kind::Session,
                "the nonce is not the one this enforcer issued",
            ));
        }
        if self.verifier_key != challenge.verifier_key {
            return Err(refused(
                Kind::Session,
                "the session is bound to another enforcer key",
            ));
        }
        let age = now.signed_duration_since(challenge.issued_at).num_seconds();
        if age < 0 || age > i64::from(challenge.nonce_lifetime_seconds) {
            return Err(refused(Kind::Session, "the nonce has expired"));
        }
        if self.max_duration_seconds == 0 || self.max_duration_seconds > MAX_SESSION_SECONDS {
            return Err(refused(
                Kind::Session,
                format!("a session may last 1 to {MAX_SESSION_SECONDS} seconds"),
            ));
        }
        if self.session_id.is_empty() || self.client_key.is_empty() {
            return Err(refused(
                Kind::Session,
                "the session id and client key are required",
            ));
        }
        Ok(())
    }
}

/// `approval/v1`: an approved operations plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalBody {
    /// Plan id in the control plane.
    pub plan_id: String,
    /// Plugin that runs the plan.
    pub plugin: String,
    /// Commands the plan may run, exactly.
    pub commands: Vec<String>,
    /// Parameters, exactly.
    #[serde(default)]
    pub parameters: serde_json::Value,
}

impl ApprovalBody {
    /// Refuse a plugin the release doesn't pin, a bundle with another digest,
    /// or a command outside the plan.
    pub fn check(&self, release: &ReleaseBody, bundle_digest: &str, command: &str) -> Result<()> {
        let pinned = release.plugins.get(&self.plugin).ok_or_else(|| {
            refused(
                Kind::Approval,
                format!("plugin '{}' is not in the signed release", self.plugin),
            )
        })?;
        if pinned != bundle_digest {
            return Err(refused(
                Kind::Approval,
                format!(
                    "plugin '{}' bundle {bundle_digest} is not the signed {pinned}",
                    self.plugin
                ),
            ));
        }
        if !self.commands.iter().any(|approved| approved == command) {
            return Err(refused(
                Kind::Approval,
                format!("command '{command}' is not in the approved plan"),
            ));
        }
        Ok(())
    }
}

/// `dynamic/v1`: one dynamic container spec from the developer's backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DynamicBody {
    /// Container name.
    pub name: String,
    /// `<repository>@sha256:<hex>`.
    pub image: String,
    /// Command, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Environment values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// CPU per replica.
    pub cpu: String,
    /// Memory per replica.
    pub memory: String,
}

impl DynamicBody {
    /// Build the container within the release's dynamic policy. Dynamic
    /// containers never get privileges, mounts or an identity.
    pub fn build(&self, release: &ReleaseBody) -> Result<BuiltWorkload> {
        let policy = release.dynamic.as_ref().ok_or_else(|| {
            refused(
                Kind::Dynamic,
                "the signed release allows no dynamic containers",
            )
        })?;
        if !is_pinned_image(&self.image) {
            return Err(refused(
                Kind::Dynamic,
                format!("image '{}' is not pinned by digest", self.image),
            ));
        }
        let repository = self
            .image
            .rsplit_once('@')
            .map(|(repository, _)| repository)
            .unwrap_or_default();
        if !policy
            .repositories
            .iter()
            .any(|allowed| allowed == repository)
        {
            return Err(refused(
                Kind::Dynamic,
                format!("repository '{repository}' is not allowed by the signed release"),
            ));
        }
        for name in self.env.keys() {
            if is_loader_env(name) || !policy.env.contains(name) {
                return Err(refused(
                    Kind::Dynamic,
                    format!("env {name} is not allowed by the signed release"),
                ));
            }
        }
        let cpu = parse_cpu_millis(&self.cpu).map_err(|reason| refused(Kind::Dynamic, reason))?;
        let memory =
            parse_memory_bytes(&self.memory).map_err(|reason| refused(Kind::Dynamic, reason))?;
        if let Some(max) = &policy.bounds.max_cpu {
            if cpu > parse_cpu_millis(max).map_err(|reason| refused(Kind::Dynamic, reason))? {
                return Err(refused(
                    Kind::Dynamic,
                    format!("cpu {} is above the signed bound {max}", self.cpu),
                ));
            }
        }
        if let Some(max) = &policy.bounds.max_memory {
            if memory > parse_memory_bytes(max).map_err(|reason| refused(Kind::Dynamic, reason))? {
                return Err(refused(
                    Kind::Dynamic,
                    format!("memory {} is above the signed bound {max}", self.memory),
                ));
            }
        }
        Ok(BuiltWorkload {
            name: self.name.clone(),
            image: self.image.clone(),
            command: self.command.clone(),
            env: self.env.clone(),
            runtime: Runtime::default(),
            volume_mount_path: None,
            identity: None,
        })
    }
}

/// `command/v1`: one signed command payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandBody {
    /// Command name.
    pub command: String,
    /// `sha256:<hex>` of the exact parameter bytes.
    pub params_sha256: String,
    /// Last moment the command may run.
    pub deadline: DateTime<Utc>,
}

impl CommandBody {
    /// Refuse a payload whose bytes, command or deadline don't match.
    pub fn check(
        &self,
        release: &ReleaseBody,
        command: &str,
        params: &[u8],
        now: DateTime<Utc>,
    ) -> Result<()> {
        if !release
            .signed_commands
            .iter()
            .any(|signed| signed == command)
            || self.command != command
        {
            return Err(refused(
                Kind::Command,
                format!("command '{command}' is not the signed command"),
            ));
        }
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(params)));
        if self.params_sha256 != digest {
            return Err(refused(
                Kind::Command,
                "the parameters differ from the signed ones",
            ));
        }
        if now > self.deadline {
            return Err(refused(Kind::Command, "the command's deadline has passed"));
        }
        Ok(())
    }
}

/// `component/v1`: one build of an enforcer or other component, signed by
/// the components key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComponentBody {
    /// Component name, such as `machine-agent`.
    pub name: String,
    /// Version, for display. The envelope's sequence orders builds.
    pub version: String,
    /// Target, such as `linux-x86_64`.
    pub target: String,
    /// `sha256:<hex>` of the artifact bytes.
    pub sha256: String,
}

impl ComponentBody {
    /// Refuse an artifact that is another component, target or set of bytes.
    pub fn check(&self, name: &str, target: &str, artifact: &[u8]) -> Result<()> {
        if self.name != name || self.target != target {
            return Err(refused(
                Kind::Component,
                format!(
                    "the statement is for {} on {}, not {name} on {target}",
                    self.name, self.target
                ),
            ));
        }
        if !is_sha256_digest(&self.sha256) {
            return Err(refused(Kind::Component, "the digest is not sha256:<hex>"));
        }
        let digest = format!("sha256:{}", hex::encode(Sha256::digest(artifact)));
        if digest != self.sha256 {
            return Err(refused(
                Kind::Component,
                "the artifact is not the signed build",
            ));
        }
        Ok(())
    }
}

fn refused(kind: Kind, reason: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::BodyRefused {
        kind: kind.as_str().to_string(),
        reason: reason.into(),
    })
}
