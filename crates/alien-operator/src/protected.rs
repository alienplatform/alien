//! Protected mode: run only releases the developer signed.
//!
//! An installer turns it on with the anchor its setup writes
//! (`OPERATOR_RELEASE_TRUST_ANCHOR`): the project, this deployment, and the
//! fingerprint of the developer's first signer list. The manager delivers the
//! signer list chain and the signed `release/v2` statements; this Operator
//! verifies both against the anchor and decides nothing on the manager's
//! word that runs code or redirects data:
//!
//! - a target's stack comes only from its verified statement; the manager may
//!   move an image to another registry, never change its digest;
//! - a release older than the one applied, or the applied sequence with other
//!   bytes, is refused, and so is a signer list older than the one accepted;
//! - the deployment configuration is rebuilt here: environment variables only
//!   for the input mappings the signed stack declares, developer inputs and
//!   configurable env only from the deployment's signed `config/v1`, deployer
//!   inputs only from this installation, bindings and endpoints only from this
//!   installation;
//! - the stored target is verified again before every deployment pass, so no
//!   other path that writes a stack can deploy it.
//!
//! Refused targets change nothing: they are logged and left unapplied.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use alien_core::release_statement::{relocate_images, signed_stack};
use alien_core::sync::TargetDeployment;
use alien_core::{
    DeploymentConfig, EnvironmentVariable, EnvironmentVariableType, EnvironmentVariablesSnapshot,
    ExternalBindings, Platform, ReleaseInfo, SignedStatement, Stack, StackInputProvider,
    StackSettings,
};
use alien_release_trust::kinds::ConfigBody;
use alien_release_trust::{Anchor, Kind, ReleaseBody, Trust};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Component name of the Operator in a release's signed components.
pub const OPERATOR_COMPONENT: &str = "operator";

/// The anchor an installation pinned.
#[derive(Debug, Clone)]
pub struct ReleaseSigning {
    /// What setup wrote.
    pub anchor: Anchor,
}

/// The newest release this Operator accepted, and the signer chain it was
/// verified with. Persisted so a restart can't be used to replay an older
/// release or an older signer list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedRelease {
    /// Its sequence.
    pub sequence: u64,
    /// `sha256:<hex>` of its statement bytes.
    pub digest: String,
    /// The Operator image digest it allows, if any.
    pub operator_digest: Option<String>,
    /// The signer list chain, oldest first.
    pub signer_lists: Vec<SignedStatement>,
    /// The sequence of the newest `config/v1` it applied, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_sequence: Option<u64>,
}

/// A release that passed verification.
#[derive(Debug, Clone)]
pub struct VerifiedRelease {
    /// The statement's sequence.
    pub sequence: u64,
    /// `sha256:<hex>` of its bytes.
    pub digest: String,
    /// The stack, read from the signed bytes.
    pub stack: Stack,
    /// The Operator image digest the release allows.
    pub operator_digest: Option<String>,
    /// The signer chain the release verified against.
    pub signer_lists: Vec<SignedStatement>,
    /// Signers whose signatures counted.
    pub signers: Vec<String>,
    /// The verified release body.
    pub body: ReleaseBody,
    /// The sequence of the `config/v1` applied with it, if any.
    pub config_sequence: Option<u64>,
}

impl From<&VerifiedRelease> for AcceptedRelease {
    fn from(verified: &VerifiedRelease) -> Self {
        Self {
            sequence: verified.sequence,
            digest: verified.digest.clone(),
            operator_digest: verified.operator_digest.clone(),
            signer_lists: verified.signer_lists.clone(),
            config_sequence: verified.config_sequence,
        }
    }
}

/// Why a target was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The release carries no signed statement.
    #[error("the release is not signed")]
    Unsigned,
    /// The statement or the signer chain failed verification.
    #[error("{code}: {message}")]
    Release {
        /// Refusal code from the verifier.
        code: String,
        /// Its message.
        message: String,
    },
    /// The release is older than the one applied, or reuses its sequence.
    #[error("the release has sequence {actual}, and {applied} is already applied")]
    OlderRelease {
        /// Applied sequence.
        applied: u64,
        /// Refused sequence.
        actual: u64,
    },
    /// The config statement is older than the one applied.
    #[error("the config has sequence {actual}, and {applied} is already applied")]
    OlderConfig {
        /// Applied sequence.
        applied: u64,
        /// Refused sequence.
        actual: u64,
    },
    /// The manager set environment variables no signed input maps.
    #[error(
        "the manager set environment variables the signed release doesn't map from an input: {}",
        .0.join(", ")
    )]
    EnvironmentVariables(Vec<String>),
    /// The signed inputs don't resolve.
    #[error("the signed release's inputs don't resolve: {0}")]
    Inputs(String),
    /// The carried stack differs from the one its statement signs.
    #[error("the stack the manager sent differs from the signed release")]
    StackChanged,
}

impl<T: alien_error::AlienErrorData + Clone + std::fmt::Debug + Serialize>
    From<alien_error::AlienError<T>> for Refusal
{
    fn from(error: alien_error::AlienError<T>) -> Self {
        Self::Release {
            code: error.code,
            message: error.message,
        }
    }
}

impl ReleaseSigning {
    /// Verify `release` and return the stack to deploy: the signed stack, with
    /// image locations taken from the carried stack where the digest matches.
    /// The carried stack must then be exactly that stack, so a manager that
    /// changes anything else is refused rather than silently corrected. Sync,
    /// air-gapped bundles and the deployment loop all check releases here.
    ///
    /// The signer chain starts from the one this Operator accepted, so a
    /// manager can deliver newer list versions but never an older one.
    pub fn verify_release(
        &self,
        platform: Platform,
        release: &ReleaseInfo,
        accepted: Option<&AcceptedRelease>,
    ) -> Result<VerifiedRelease, Refusal> {
        let signed = release.signed.as_ref().ok_or(Refusal::Unsigned)?;
        let chain = match accepted {
            Some(accepted) if !accepted.signer_lists.is_empty() => &accepted.signer_lists,
            _ => &release.signer_lists,
        };
        let mut trust = Trust::new(self.anchor.clone(), chain)?;
        trust.extend_chain(&release.signer_lists)?;
        let verified = trust.verify::<ReleaseBody>(signed, Kind::Release, Utc::now())?;
        verified.body.validate()?;
        let stack = signed_stack(&verified.body, platform)?;
        if relocate_images(&stack, &release.stack) != release.stack {
            return Err(Refusal::StackChanged);
        }
        Ok(VerifiedRelease {
            sequence: verified.sequence,
            digest: verified.digest,
            stack,
            operator_digest: verified.body.components.get(OPERATOR_COMPONENT).cloned(),
            signer_lists: trust.chain().statements().to_vec(),
            signers: verified.signers.into_iter().collect(),
            body: verified.body,
            config_sequence: None,
        })
    }

    /// The target to store for `target`, and the release it verified. Refuses
    /// a release older than `accepted`.
    pub fn protect_target(
        &self,
        platform: Platform,
        local_input_values: &serde_json::Map<String, serde_json::Value>,
        accepted: Option<&AcceptedRelease>,
        target: &TargetDeployment,
    ) -> Result<(TargetDeployment, VerifiedRelease), Refusal> {
        let verified = self.verify_release(platform, &target.release_info, accepted)?;
        if let Some(accepted) = accepted {
            let older = verified.sequence < accepted.sequence
                || (verified.sequence == accepted.sequence && verified.digest != accepted.digest);
            if older {
                return Err(Refusal::OlderRelease {
                    applied: accepted.sequence,
                    actual: verified.sequence,
                });
            }
        }
        let mut verified = verified;
        let signed_config = match &target.signed_config {
            None => ConfigBody::default(),
            Some(signed) => {
                let trust = Trust::new(self.anchor.clone(), &verified.signer_lists)?;
                let config = trust.verify::<ConfigBody>(signed, Kind::Config, Utc::now())?;
                if let Some(applied) = accepted.and_then(|accepted| accepted.config_sequence) {
                    if config.sequence < applied {
                        return Err(Refusal::OlderConfig {
                            applied,
                            actual: config.sequence,
                        });
                    }
                }
                config.body.check(&verified.body)?;
                verified.config_sequence = Some(config.sequence);
                config.body
            }
        };
        let config = protect_config(
            platform,
            &target.release_info.stack,
            local_input_values,
            &signed_config,
            &target.config,
        )?;
        Ok((
            TargetDeployment {
                release_info: target.release_info.clone(),
                config,
                signed_config: target.signed_config.clone(),
            },
            verified,
        ))
    }
}

/// Environment variable names the signed stack's inputs may set.
fn input_environment_names(stack: &Stack) -> BTreeSet<&str> {
    stack
        .inputs
        .iter()
        .flat_map(|input| input.env.iter().map(|mapping| mapping.name.as_str()))
        .collect()
}

/// Whether the deployer, rather than the developer, supplies `input`'s value.
fn deployer_supplied(input: &alien_core::StackInputDefinition) -> bool {
    input.provided_by.contains(&StackInputProvider::Deployer)
}

/// Developer inputs, whose values come from signed config, for the startup log.
pub fn developer_inputs(stack: &Stack) -> Vec<&str> {
    stack
        .inputs
        .iter()
        .filter(|input| !deployer_supplied(input))
        .map(|input| input.id.as_str())
        .collect()
}

/// Rebuild the manager's configuration from what this installation and the
/// signed stack allow. Every field is decided here, so a field added later must
/// be decided too.
fn protect_config(
    platform: Platform,
    stack: &Stack,
    local_input_values: &serde_json::Map<String, serde_json::Value>,
    signed_config: &ConfigBody,
    manager: &DeploymentConfig,
) -> Result<DeploymentConfig, Refusal> {
    let mut allowed = input_environment_names(stack);
    allowed.extend(signed_config.env.keys().map(String::as_str));
    let unexpected: BTreeSet<String> = manager
        .environment_variables
        .variables
        .iter()
        .filter(|variable| !allowed.contains(variable.name.as_str()))
        .map(|variable| variable.name.clone())
        .collect();
    if !unexpected.is_empty() {
        return Err(Refusal::EnvironmentVariables(
            unexpected.into_iter().collect(),
        ));
    }

    // Deployer inputs come from this installation, developer inputs from the
    // deployment's signed config, never from the manager. The developer
    // declared where each one is used, so an input can only reach the
    // variables the signed stack maps it to.
    let input_values: HashMap<String, serde_json::Value> = stack
        .inputs
        .iter()
        .filter_map(|input| {
            let value = if deployer_supplied(input) {
                local_input_values.get(&input.id)
            } else {
                signed_config.inputs.get(&input.id)
            };
            value.map(|value| (input.id.clone(), value.clone()))
        })
        .collect();
    let mut variables = alien_core::resolve_stack_input_environment_variables(
        &stack.inputs,
        &input_values,
        platform,
    )
    .map_err(|error| Refusal::Inputs(error.message))?;
    // Configurable env the signed config sets, which the release already
    // limited to names it lists.
    let configured: BTreeMap<&String, &String> = signed_config.env.iter().collect();
    variables.retain(|variable| !configured.contains_key(&variable.name));
    variables.extend(configured.into_iter().map(|(name, value)| EnvironmentVariable {
        name: name.clone(),
        value: value.clone(),
        var_type: EnvironmentVariableType::Plain,
        target_resources: None,
    }));

    Ok(DeploymentConfig {
        setup_run: manager.setup_run,
        setup_run_id: manager.setup_run_id.clone(),
        environment_variables: EnvironmentVariablesSnapshot {
            variables,
            hash: manager.environment_variables.hash.clone(),
            created_at: manager.environment_variables.created_at.clone(),
        },
        input_values,
        // Secret inputs are resolved from the deployment's own vault.
        stored_secret_input_ids: manager.stored_secret_input_ids.clone(),
        // Timing, telemetry, pull credentials and deletion: an outage at worst.
        allow_frozen_changes: manager.allow_frozen_changes,
        monitoring: manager.monitoring.clone(),
        manager_url: manager.manager_url.clone(),
        deployment_token: manager.deployment_token.clone(),
        volume_restores: manager.volume_restores.clone(),
        deletion_policy: manager.deletion_policy,
        // Where data goes and what the deployment is called come from this
        // installation; the deployment loop fills these from its settings.
        deployment_name: None,
        stack_settings: StackSettings::default(),
        external_bindings: ExternalBindings::default(),
        base_platform: None,
        label_domain: None,
        observe_label_selector: None,
        observe_all_namespaces: false,
        public_endpoints: None,
        // Identities, endpoints and registries a manager would choose. A
        // protected installation runs without them.
        management_config: None,
        gcp_federation_scope: None,
        compute_backend: None,
        domain_metadata: None,
        native_image_host: None,
        release_statements: None,
    })
}

/// A protected configuration for tests: an anchor pinning a one-key signer
/// list for `project`.
#[cfg(test)]
pub(crate) fn test_signing(project: &str) -> ReleaseSigning {
    let key = alien_release_trust::SoftwareKey::from_secret_bytes(&[21; 32]).unwrap();
    let list = test_signer_list(project, &key);
    ReleaseSigning {
        anchor: Anchor {
            project: project.to_string(),
            deployment: format!("dep_{project}"),
            protected: true,
            signing: alien_release_trust::fingerprint(&list.open().unwrap().digest),
            components: alien_release_trust::SoftwareKey::from_secret_bytes(&[22; 32])
                .unwrap()
                .public_key()
                .as_str()
                .to_string(),
            session_approval: Default::default(),
            inputs: None,
            resource_prefix: None,
        },
    }
}

/// Version 1 of a signer list with `key` as its one releaser and admin.
#[cfg(test)]
pub(crate) fn test_signer_list(
    project: &str,
    key: &alien_release_trust::SoftwareKey,
) -> SignedStatement {
    use alien_release_trust::{Envelope, Role, Signer, SignerKind, SignerListBody};
    let envelope = Envelope {
        kind: Kind::SignerList.as_str().to_string(),
        project: Some(project.to_string()),
        deployment: None,
        sequence: 1,
        not_after: None,
        signer_list_version: 1,
        body: SignerListBody {
            previous: None,
            admin_threshold: 1,
            signers: vec![Signer {
                id: "ci".to_string(),
                key: key.public_key().as_str().to_string(),
                kind: SignerKind::Ci,
                roles: vec![Role::Release, Role::Admin],
                label: None,
            }],
            deny: Vec::new(),
        },
    };
    SignedStatement::new(&envelope.to_payload().unwrap())
        .sign_with("ci", key)
        .unwrap()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use alien_core::release_statement::{release_body, ReleaseOptions};
    use alien_core::{
        Container, ContainerCode, EnvironmentVariable, EnvironmentVariableType, ResourceLifecycle,
        ResourceSpec, StackInputDefinition,
    };
    use alien_release_trust::{Envelope, SoftwareKey};

    const DIGEST: &str = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    pub(crate) fn key() -> SoftwareKey {
        SoftwareKey::from_secret_bytes(&[21; 32]).unwrap()
    }

    pub(crate) fn signer_list() -> SignedStatement {
        test_signer_list("shop", &key())
    }

    pub(crate) fn signing() -> ReleaseSigning {
        test_signing("shop")
    }

    pub(crate) fn input(id: &str, provider: &str, env: &str) -> StackInputDefinition {
        serde_json::from_value(serde_json::json!({
            "id": id, "kind": "string", "providedBy": [provider], "required": false,
            "label": id, "description": id,
            "env": [{ "name": env, "targetResources": ["api"] }],
        }))
        .unwrap()
    }

    pub(crate) fn stack(image: &str) -> Stack {
        let container = Container::new("api".to_string())
            .code(ContainerCode::Image {
                image: image.to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("execution".to_string())
            .build();
        let mut stack = Stack::new("shop".to_string())
            .add(container, ResourceLifecycle::Live)
            .build();
        stack.inputs = vec![
            input("logLevel", "developer", "LOG_LEVEL"),
            input("databaseUrl", "deployer", "DATABASE_URL"),
        ];
        stack
    }

    pub(crate) fn signed_release(sequence: u64, stack: &Stack) -> ReleaseInfo {
        let envelope = Envelope {
            kind: Kind::Release.as_str().to_string(),
            project: Some("shop".to_string()),
            deployment: None,
            sequence,
            not_after: None,
            signer_list_version: 1,
            body: release_body(Platform::Kubernetes, stack, &ReleaseOptions::default()).unwrap(),
        };
        let signed = SignedStatement::new(&envelope.to_payload().unwrap())
            .sign_with("ci", &key())
            .unwrap();
        ReleaseInfo {
            release_id: Some(format!("rel_{sequence}")),
            version: None,
            description: None,
            stack: stack.clone(),
            signed: Some(signed),
            signer_lists: vec![signer_list()],
        }
    }

    pub(crate) fn variable(name: &str, value: &str) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_string(),
            value: value.to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        }
    }

    pub(crate) fn target(release_info: ReleaseInfo, variables: Vec<EnvironmentVariable>) -> TargetDeployment {
        TargetDeployment {
            signed_config: None,
            release_info,
            config: DeploymentConfig::builder()
                .stack_settings(StackSettings::default())
                .environment_variables(EnvironmentVariablesSnapshot {
                    variables,
                    hash: String::new(),
                    created_at: String::new(),
                })
                .input_values(HashMap::from([
                    ("logLevel".to_string(), serde_json::json!("debug")),
                    (
                        "databaseUrl".to_string(),
                        serde_json::json!("postgres://attacker.example/db"),
                    ),
                ]))
                .allow_frozen_changes(false)
                .external_bindings(ExternalBindings::default())
                .build(),
        }
    }

    pub(crate) fn local_inputs() -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_value(serde_json::json!({ "databaseUrl": "postgres://db.internal/shop" }))
            .unwrap()
    }

    pub(crate) fn pinned() -> String {
        format!("registry.example.com/shop/api@{DIGEST}")
    }

    #[test]
    fn a_stack_other_than_the_signed_one_is_refused() {
        let signed = stack(&pinned());
        let release = signed_release(5, &signed);
        let (protected, verified) = signing()
            .protect_target(
                Platform::Kubernetes,
                &local_inputs(),
                None,
                &target(release.clone(), vec![]),
            )
            .unwrap();
        assert_eq!(protected.release_info.stack, signed);
        assert_eq!(verified.sequence, 5);

        let mut moved = release.clone();
        moved.stack = stack(&format!("mirror.example.com/shop/api@{DIGEST}"));
        signing()
            .protect_target(
                Platform::Kubernetes,
                &local_inputs(),
                None,
                &target(moved, vec![]),
            )
            .expect("an image may move to another registry under the same digest");

        let mut changed = release;
        changed.stack = stack(&format!(
            "registry.example.com/shop/api@sha256:{}",
            "e".repeat(64)
        ));
        assert_eq!(
            signing()
                .protect_target(
                    Platform::Kubernetes,
                    &local_inputs(),
                    None,
                    &target(changed, vec![])
                )
                .unwrap_err(),
            Refusal::StackChanged
        );
    }

    #[test]
    fn an_unsigned_target_is_refused() {
        let mut release = signed_release(5, &stack(&pinned()));
        release.signed = None;
        assert_eq!(
            signing()
                .protect_target(
                    Platform::Kubernetes,
                    &local_inputs(),
                    None,
                    &target(release, vec![])
                )
                .unwrap_err(),
            Refusal::Unsigned
        );
    }

    #[test]
    fn an_older_release_is_refused() {
        let accepted = AcceptedRelease {
            config_sequence: None,
            sequence: 6,
            digest: "sha256:other".to_string(),
            operator_digest: None,
            signer_lists: vec![signer_list()],
        };
        let release = signed_release(5, &stack(&pinned()));
        assert_eq!(
            signing()
                .protect_target(
                    Platform::Kubernetes,
                    &local_inputs(),
                    Some(&accepted),
                    &target(release, vec![])
                )
                .unwrap_err(),
            Refusal::OlderRelease {
                applied: 6,
                actual: 5
            }
        );
    }

    #[test]
    fn manager_environment_variables_are_refused_by_name() {
        let release = signed_release(5, &stack(&pinned()));
        let refusal = signing()
            .protect_target(
                Platform::Kubernetes,
                &local_inputs(),
                None,
                &target(
                    release,
                    vec![
                        variable("LD_PRELOAD", "/tmp/evil.so"),
                        variable("LOG_LEVEL", "debug"),
                        variable("NODE_OPTIONS", "--import=data:text/javascript,evil"),
                    ],
                ),
            )
            .unwrap_err();
        assert_eq!(
            refusal,
            Refusal::EnvironmentVariables(vec![
                "LD_PRELOAD".to_string(),
                "NODE_OPTIONS".to_string()
            ])
        );
    }

    /// A `config/v1` for the test deployment, signed by the release key.
    pub(crate) fn signed_config(sequence: u64, deployment: &str, body: serde_json::Value) -> SignedStatement {
        let envelope = Envelope {
            kind: Kind::Config.as_str().to_string(),
            project: Some("shop".to_string()),
            deployment: Some(deployment.to_string()),
            sequence,
            not_after: None,
            signer_list_version: 1,
            body,
        };
        SignedStatement::new(&envelope.to_payload().unwrap())
            .sign_with("ci", &key())
            .unwrap()
    }

    pub(crate) fn values(target: &TargetDeployment) -> HashMap<String, String> {
        target
            .config
            .environment_variables
            .variables
            .iter()
            .map(|variable| (variable.name.clone(), variable.value.clone()))
            .collect()
    }

    #[test]
    fn deployer_inputs_come_from_the_installation_and_developer_inputs_from_signed_config() {
        let release = signed_release(5, &stack(&pinned()));
        // The manager sets both inputs; neither value may run.
        let mut target = target(
            release,
            vec![
                variable("LOG_LEVEL", "debug"),
                variable("DATABASE_URL", "postgres://attacker.example/db"),
            ],
        );
        let (protected, _) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), None, &target)
            .unwrap();
        let unsigned = values(&protected);
        assert_eq!(unsigned["DATABASE_URL"], "postgres://db.internal/shop");
        assert!(
            !unsigned.contains_key("LOG_LEVEL"),
            "without signed config, the manager's developer input value is dropped: {unsigned:?}"
        );
        assert_eq!(
            protected.config.input_values["databaseUrl"],
            serde_json::json!("postgres://db.internal/shop")
        );

        target.signed_config = Some(signed_config(
            3,
            "dep_shop",
            serde_json::json!({"inputs": {"logLevel": "warn"}}),
        ));
        let (protected, verified) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), None, &target)
            .unwrap();
        assert_eq!(values(&protected)["LOG_LEVEL"], "warn", "the signed value runs");
        assert_eq!(verified.config_sequence, Some(3));

        // An older config can't come back once a newer one was applied.
        let accepted = AcceptedRelease::from(&verified);
        target.signed_config = Some(signed_config(
            2,
            "dep_shop",
            serde_json::json!({"inputs": {"logLevel": "trace"}}),
        ));
        assert_eq!(
            signing()
                .protect_target(Platform::Kubernetes, &local_inputs(), Some(&accepted), &target)
                .unwrap_err(),
            Refusal::OlderConfig {
                applied: 3,
                actual: 2
            }
        );

        // Config signed for another deployment, or setting what the release
        // doesn't make configurable, is refused.
        target.signed_config = Some(signed_config(
            4,
            "dep_other",
            serde_json::json!({"inputs": {"logLevel": "warn"}}),
        ));
        assert!(signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), None, &target)
            .is_err());
        target.signed_config = Some(signed_config(
            4,
            "dep_shop",
            serde_json::json!({"inputs": {"databaseUrl": "postgres://attacker.example/db"}}),
        ));
        assert!(signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), None, &target)
            .is_err());
    }

    #[test]
    fn manager_identities_and_bindings_are_dropped() {
        let release = signed_release(5, &stack(&pinned()));
        let mut target = target(release, vec![]);
        target.config.native_image_host = Some("attacker.example".to_string());
        target.config.observe_label_selector = Some("app=other".to_string());
        target.config.external_bindings.insert(
            "data",
            alien_core::ExternalBinding::Storage(alien_core::StorageBinding::s3("attacker-bucket")),
        );

        let (protected, _) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), None, &target)
            .unwrap();
        assert_eq!(protected.config.native_image_host, None);
        assert_eq!(protected.config.observe_label_selector, None);
        assert!(protected.config.external_bindings.0.is_empty());
    }

    #[test]
    fn a_stack_written_after_verification_is_caught_before_deploying() {
        let signed = stack(&pinned());
        let release = signed_release(5, &signed);
        signing()
            .verify_release(Platform::Kubernetes, &release, None)
            .expect("the stack as signed is deployable");

        let mut changed = release.clone();
        changed.stack = stack(&format!(
            "registry.example.com/shop/api@sha256:{}",
            "e".repeat(64)
        ));
        assert_eq!(
            signing()
                .verify_release(Platform::Kubernetes, &changed, None)
                .unwrap_err(),
            Refusal::StackChanged
        );
    }
}
