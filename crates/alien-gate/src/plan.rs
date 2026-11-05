//! From a request and the trusted state to the function the gate writes.
//!
//! Nothing here touches the network: the caller supplies the state it loaded
//! and the deployer inputs it read, and gets back the function to write and
//! the state to store once the write succeeds.

use std::collections::BTreeMap;

use alien_error::{AlienError, Context};
use alien_release_trust::kinds::ConfigBody;
use alien_release_trust::release::{is_pinned_image, Runtime, WorkloadKind};
use alien_release_trust::{
    build_workload, Anchor, Kind, ReleaseBody, SignedStatement, Trust, WorkloadRequest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use crate::state::GateState;

/// Floor key for releases.
pub const RELEASE_FLOOR: &str = "release/v2";
/// Floor key for config statements.
pub const CONFIG_FLOOR: &str = "config/v1";

/// What the control plane asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    /// Write the function, then read it back.
    Apply,
    /// Only read the function back and report whether it matches.
    Check,
}

/// One call to the gate. Unknown fields are refused: the control plane can't
/// pass a spec, only statements and the values the release lets it provide.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GateRequest {
    /// Apply or check.
    pub action: Action,
    /// The workload in the release.
    pub workload: String,
    /// The release, as the developer signed it.
    pub signed_release: SignedStatement,
    /// This deployment's config, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_config: Option<SignedStatement>,
    /// Newer signer list versions to accept.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signer_lists: Vec<SignedStatement>,
    /// Values for the env names the release lets the control plane provide.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Where this deployment's copy of the signed image lives. Its digest
    /// must be the signed one and its repository in this deployment's
    /// registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

/// What setup configures the gate with. The control plane can change none
/// of it: setup creates the gate and only the gate's own role can update it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateConfig {
    /// The deployment's anchor.
    pub anchor: Anchor,
    /// Functions the gate owns are named `<prefix><workload>`.
    pub function_prefix: String,
    /// Repository prefix a relocated image must start with.
    pub registry: String,
    /// What each permission profile runs as: an IAM role ARN on AWS, a
    /// service account email on GCP.
    pub roles: BTreeMap<String, String>,
}

/// The function the gate writes, built from the signed workload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionSpec {
    /// Function name.
    pub name: String,
    /// Image URI, pinned by digest.
    pub image_uri: String,
    /// The signed digest.
    pub digest: String,
    /// The whole environment.
    pub env: BTreeMap<String, String>,
    /// Memory in MiB.
    pub memory_mb: u32,
    /// Timeout in seconds.
    pub timeout_seconds: u32,
    /// `x86_64` or `arm64`.
    pub architecture: String,
    /// What it runs as: the execution role ARN on AWS, the service account
    /// email on GCP.
    pub run_as: String,
}

/// A planned write and the state to store once it succeeds.
#[derive(Debug, Clone)]
pub struct Planned {
    /// The function.
    pub function: FunctionSpec,
    /// The state after this call.
    pub state: GateState,
}

/// Verify the request's statements and build the function from them.
pub fn plan(
    config: &GateConfig,
    state: &GateState,
    request: &GateRequest,
    deployer_inputs: &BTreeMap<String, String>,
    now: DateTime<Utc>,
) -> Result<Planned> {
    let refuse = |reason: String| AlienError::new(ErrorData::Refused { reason });

    let base = if state.signer_lists.is_empty() {
        &request.signer_lists
    } else {
        &state.signer_lists
    };
    let mut trust = Trust::new(config.anchor.clone(), base).context(ErrorData::Refused {
        reason: "the signer list was refused".to_string(),
    })?;
    trust
        .extend_chain(&request.signer_lists)
        .context(ErrorData::Refused {
            reason: "a newer signer list was refused".to_string(),
        })?;

    let release = trust
        .verify::<ReleaseBody>(&request.signed_release, Kind::Release, now)
        .context(ErrorData::Refused {
            reason: "the release was refused".to_string(),
        })?;
    state
        .floors
        .check_current(RELEASE_FLOOR, release.sequence)
        .context(ErrorData::Refused {
            reason: "the release is older than one already applied".to_string(),
        })?;
    release.body.validate().context(ErrorData::Refused {
        reason: "the release breaks the release rules".to_string(),
    })?;

    let mut floors = state.floors.clone();
    floors.raise(RELEASE_FLOOR, release.sequence);
    let config_env = match &request.signed_config {
        None => BTreeMap::new(),
        Some(signed) => {
            let config = trust
                .verify::<ConfigBody>(signed, Kind::Config, now)
                .context(ErrorData::Refused {
                    reason: "the config was refused".to_string(),
                })?;
            state
                .floors
                .check_current(CONFIG_FLOOR, config.sequence)
                .context(ErrorData::Refused {
                    reason: "the config is older than one already applied".to_string(),
                })?;
            config
                .body
                .check(&release.body)
                .context(ErrorData::Refused {
                    reason: "the config sets what the release doesn't let it".to_string(),
                })?;
            floors.raise(CONFIG_FLOOR, config.sequence);
            config.body.env
        }
    };

    let workload = release
        .body
        .workload(&request.workload)
        .context(ErrorData::Refused {
            reason: format!("workload '{}' is not in the release", request.workload),
        })?;
    if workload.kind != WorkloadKind::Function {
        return Err(refuse(format!(
            "workload '{}' is not a function",
            workload.name
        )));
    }
    let settings = workload.function.as_ref().ok_or_else(|| {
        refuse(format!(
            "workload '{}' has no signed function settings",
            workload.name
        ))
    })?;
    // The gate writes no image config, layers or mounts, so a release that
    // needs them is refused rather than written without them.
    if workload.command.is_some() {
        return Err(refuse(format!(
            "workload '{}' overrides the image command, which the gate doesn't write",
            workload.name
        )));
    }
    if workload.runtime != Runtime::default() || workload.volume_mount_path.is_some() {
        return Err(refuse(format!(
            "workload '{}' asks for runtime privileges or mounts a function can't have",
            workload.name
        )));
    }
    let identity = workload.identity.as_deref().ok_or_else(|| {
        refuse(format!(
            "workload '{}' names no permission profile",
            workload.name
        ))
    })?;
    let run_as = config.roles.get(identity).ok_or_else(|| {
        refuse(format!(
            "permission profile '{identity}' has no role in this deployment"
        ))
    })?;

    let built = build_workload(
        &release.body,
        &workload.name,
        &config_env,
        &WorkloadRequest {
            env: request.env.clone(),
            deployer_inputs: deployer_inputs.clone(),
            ..Default::default()
        },
    )
    .context(ErrorData::Refused {
        reason: "the control plane's values don't fit the release".to_string(),
    })?;

    if !is_pinned_image(&built.image) {
        return Err(refuse(format!("image {} is not pinned", built.image)));
    }
    let (_, digest) = built
        .image
        .rsplit_once('@')
        .expect("a pinned image has a digest");
    let image_uri = match &request.image {
        None => built.image.clone(),
        Some(copy) => {
            let (repository, copy_digest) = copy
                .rsplit_once('@')
                .ok_or_else(|| refuse(format!("image {copy} is not pinned by digest")))?;
            if copy_digest != digest {
                return Err(refuse(format!(
                    "image {copy} is not the signed digest {digest}"
                )));
            }
            if !repository.starts_with(&config.registry) {
                return Err(refuse(format!(
                    "image {copy} is outside this deployment's registry"
                )));
            }
            copy.clone()
        }
    };

    let architecture = match settings.architecture.as_deref() {
        None | Some("x86_64") | Some("amd64") => "x86_64",
        Some("arm64") => "arm64",
        Some(other) => return Err(refuse(format!("architecture '{other}' is not known"))),
    };

    Ok(Planned {
        function: FunctionSpec {
            name: format!("{}{}", config.function_prefix, workload.name),
            image_uri,
            digest: digest.to_string(),
            env: built.env,
            memory_mb: settings.memory_mb,
            timeout_seconds: settings.timeout_seconds,
            architecture: architecture.to_string(),
            run_as: run_as.clone(),
        },
        state: GateState {
            signer_lists: trust.chain().statements().to_vec(),
            floors,
        },
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use alien_release_trust::envelope::Envelope;
    use alien_release_trust::release::{Bounds, Configurable, FunctionSettings, ProvidedEnv};
    use alien_release_trust::{fingerprint, Role, Signer, SignerKind, SignerListBody, SoftwareKey};
    use alien_release_trust::{SessionApproval, Workload};

    pub const DIGEST: &str =
        "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    pub const REGISTRY: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/acme-";

    pub fn key(seed: u8) -> SoftwareKey {
        let mut secret = [seed; 32];
        secret[0] = 1;
        SoftwareKey::from_secret_bytes(&secret).unwrap()
    }

    pub fn statement<B: Serialize>(
        kind: Kind,
        deployment: Option<&str>,
        sequence: u64,
        body: B,
    ) -> SignedStatement {
        SignedStatement::new(
            &Envelope {
                kind: kind.as_str().to_string(),
                project: Some("shop".to_string()),
                deployment: deployment.map(str::to_string),
                sequence,
                not_after: None,
                signer_list_version: 1,
                body,
            }
            .to_payload()
            .unwrap(),
        )
    }

    pub fn signer_list() -> SignedStatement {
        let body = SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![
                Signer {
                    id: "ci".into(),
                    key: key(1).public_key().as_str().into(),
                    kind: SignerKind::Ci,
                    roles: vec![Role::Release],
                    label: None,
                },
                Signer {
                    id: "admin".into(),
                    key: key(2).public_key().as_str().into(),
                    kind: SignerKind::Software,
                    roles: vec![Role::Admin, Role::Release],
                    label: None,
                },
            ],
            deny: Vec::new(),
        };
        statement(Kind::SignerList, None, 1, body)
            .sign_with("admin", &key(2))
            .unwrap()
    }

    pub fn workload() -> Workload {
        Workload {
            name: "api".into(),
            kind: WorkloadKind::Function,
            image: format!("ghcr.io/shop/api@{DIGEST}"),
            command: None,
            env: BTreeMap::from([("MODE".into(), "signed".into())]),
            provided_env: ProvidedEnv {
                names: vec![],
                prefixes: vec!["ALIEN_".into()],
            },
            configurable_env: vec!["LOG_LEVEL".into()],
            deployer_env: BTreeMap::new(),
            secrets: vec![],
            runtime: Runtime::default(),
            volume_mount_path: None,
            identity: Some("execution".into()),
            bounds: Bounds::default(),
            function: Some(FunctionSettings {
                memory_mb: 512,
                timeout_seconds: 30,
                architecture: None,
            }),
        }
    }

    pub fn release_with(sequence: u64, workload: Workload) -> SignedStatement {
        let body = ReleaseBody {
            platform: "aws".into(),
            workloads: vec![workload],
            configurable: Configurable {
                env: vec!["LOG_LEVEL".into()],
                inputs: vec![],
            },
            components: BTreeMap::new(),
            remote_access: vec![],
            dynamic: None,
            plugins: BTreeMap::new(),
            signed_commands: vec![],
            stack: None,
        };
        statement(Kind::Release, None, sequence, body)
            .sign_with("ci", &key(1))
            .unwrap()
    }

    pub fn config() -> GateConfig {
        GateConfig {
            anchor: Anchor {
                project: "shop".into(),
                deployment: "acme".into(),
                protected: true,
                signing: fingerprint(&signer_list().open().unwrap().digest),
                components: key(3).public_key().as_str().into(),
                session_approval: SessionApproval::default(),
                inputs: None,
                resource_prefix: Some("acme".into()),
            },
            function_prefix: "acme-fn-".into(),
            registry: REGISTRY.into(),
            roles: BTreeMap::from([(
                "execution".into(),
                "arn:aws:iam::123456789012:role/acme-execution".into(),
            )]),
        }
    }

    pub fn request(release: SignedStatement) -> GateRequest {
        GateRequest {
            action: Action::Apply,
            workload: "api".into(),
            signed_release: release,
            signed_config: None,
            signer_lists: vec![signer_list()],
            env: BTreeMap::from([("ALIEN_DEPLOYMENT_ID".into(), "dep_1".into())]),
            image: Some(format!("{REGISTRY}api@{DIGEST}")),
        }
    }

    fn refused(request: &GateRequest) -> String {
        plan(
            &config(),
            &GateState::default(),
            request,
            &BTreeMap::new(),
            Utc::now(),
        )
        .expect_err("must be refused")
        .to_string()
    }

    #[test]
    fn the_function_is_built_from_the_signed_workload() {
        let planned = plan(
            &config(),
            &GateState::default(),
            &request(release_with(10, workload())),
            &BTreeMap::new(),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(
            planned.function,
            FunctionSpec {
                name: "acme-fn-api".into(),
                image_uri: format!("{REGISTRY}api@{DIGEST}"),
                digest: DIGEST.into(),
                env: BTreeMap::from([
                    ("ALIEN_DEPLOYMENT_ID".into(), "dep_1".into()),
                    ("MODE".into(), "signed".into()),
                ]),
                memory_mb: 512,
                timeout_seconds: 30,
                architecture: "x86_64".into(),
                run_as: "arn:aws:iam::123456789012:role/acme-execution".into(),
            }
        );
        assert_eq!(planned.state.floors.get(RELEASE_FLOOR), Some(10));
        assert_eq!(planned.state.signer_lists.len(), 1);
    }

    #[test]
    fn a_spec_from_the_control_plane_is_refused() {
        let mut value = serde_json::to_value(request(release_with(10, workload()))).unwrap();
        value["environment"] = serde_json::json!({"Variables": {"MODE": "attacker"}});
        let error = serde_json::from_value::<GateRequest>(value).unwrap_err();
        assert!(
            error.to_string().contains("unknown field `environment`"),
            "{error}"
        );

        let mut value = serde_json::to_value(request(release_with(10, workload()))).unwrap();
        value["roleArn"] = serde_json::json!("arn:aws:iam::123456789012:role/admin");
        assert!(serde_json::from_value::<GateRequest>(value).is_err());
    }

    #[test]
    fn values_the_release_does_not_allow_are_refused() {
        let mut changed = request(release_with(10, workload()));
        changed.env.insert("MODE".into(), "attacker".into());
        assert!(refused(&changed).contains("don't fit the release"));

        let mut loader = request(release_with(10, workload()));
        loader
            .env
            .insert("NODE_OPTIONS".into(), "--require /tmp/x.js".into());
        assert!(refused(&loader).contains("don't fit the release"));

        let mut configurable = request(release_with(10, workload()));
        configurable.env.insert("LOG_LEVEL".into(), "trace".into());
        assert!(refused(&configurable).contains("don't fit the release"));
    }

    #[test]
    fn only_the_signed_digest_in_this_registry_is_written() {
        let other = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
        let mut digest = request(release_with(10, workload()));
        digest.image = Some(format!("{REGISTRY}api@{other}"));
        assert!(refused(&digest).contains("is not the signed digest"));

        let mut registry = request(release_with(10, workload()));
        registry.image = Some(format!(
            "999999999999.dkr.ecr.us-east-1.amazonaws.com/x@{DIGEST}"
        ));
        assert!(refused(&registry).contains("outside this deployment's registry"));

        let mut tag = request(release_with(10, workload()));
        tag.image = Some(format!("{REGISTRY}api:latest"));
        assert!(refused(&tag).contains("not pinned by digest"));
    }

    #[test]
    fn an_older_release_or_another_signer_is_refused() {
        let first = plan(
            &config(),
            &GateState::default(),
            &request(release_with(20, workload())),
            &BTreeMap::new(),
            Utc::now(),
        )
        .unwrap();
        let error = plan(
            &config(),
            &first.state,
            &request(release_with(10, workload())),
            &BTreeMap::new(),
            Utc::now(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("older than one already applied"), "{error}");
        // The same release again is fine: the control plane re-applies it.
        plan(
            &config(),
            &first.state,
            &request(release_with(20, workload())),
            &BTreeMap::new(),
            Utc::now(),
        )
        .unwrap();

        let by_attacker = statement(Kind::Release, None, 30, {
            let opened = release_with(30, workload()).open().unwrap();
            opened.body::<ReleaseBody>().unwrap()
        })
        .sign_with("ci", &key(9))
        .unwrap();
        assert!(refused(&request(by_attacker)).contains("release was refused"));
    }

    #[test]
    fn what_a_function_cannot_have_is_refused() {
        let mut command = workload();
        command.command = Some(vec!["/bin/sh".into()]);
        assert!(
            refused(&request(release_with(10, command))).contains("overrides the image command")
        );

        let mut container = workload();
        container.kind = WorkloadKind::Container;
        container.function = None;
        assert!(refused(&request(release_with(10, container))).contains("is not a function"));

        let mut unknown_role = workload();
        unknown_role.identity = Some("admin".into());
        assert!(refused(&request(release_with(10, unknown_role))).contains("has no role"));
    }

    #[test]
    fn signed_config_and_deployer_inputs_reach_the_function() {
        let mut with_input = workload();
        with_input
            .deployer_env
            .insert("REGION".into(), "region".into());
        let mut request = request(release_with(10, with_input));
        request.signed_config = Some(
            statement(
                Kind::Config,
                Some("acme"),
                3,
                ConfigBody {
                    env: BTreeMap::from([("LOG_LEVEL".into(), "debug".into())]),
                    inputs: BTreeMap::new(),
                },
            )
            .sign_with("ci", &key(1))
            .unwrap(),
        );
        request.env.insert("REGION".into(), "attacker".into());
        let planned = plan(
            &config(),
            &GateState::default(),
            &request,
            &BTreeMap::from([("region".into(), "eu".into())]),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(planned.function.env["LOG_LEVEL"], "debug");
        assert_eq!(planned.function.env["REGION"], "eu");
        assert_eq!(planned.state.floors.get(CONFIG_FLOOR), Some(3));
    }
}
