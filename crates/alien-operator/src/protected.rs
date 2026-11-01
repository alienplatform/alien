//! Protected mode: run only releases the developer signed.
//!
//! An installer turns it on by pinning the developer's public keys and project
//! (`OPERATOR_RELEASE_SIGNING_PUBLIC_KEYS`, `OPERATOR_RELEASE_SIGNING_PROJECT`).
//! From then on the manager decides which signed release runs and when, and
//! nothing else that runs code or redirects data:
//!
//! - a target's stack comes only from its verified statement; the manager may
//!   move an image to another registry, never change its digest;
//! - a release older than the one applied, or the applied sequence with other
//!   bytes, is refused;
//! - the deployment configuration is rebuilt here: environment variables only
//!   for the input mappings the signed stack declares, deployer inputs only from
//!   this installation, bindings and endpoints only from this installation;
//! - the stored target is verified again before every deployment pass, so no
//!   other path that writes a stack can deploy it.
//!
//! Refused targets change nothing: they are logged and left unapplied.

use std::collections::{BTreeSet, HashMap};

use alien_core::release_signature::{
    relocate_images, ReleaseRejection, TrustedKeys, VerifiedRelease,
};
use alien_core::sync::TargetDeployment;
use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform, ReleaseInfo, Stack,
    StackInputProvider, StackSettings,
};
use serde::{Deserialize, Serialize};

/// The keys and project an installation pinned.
#[derive(Debug, Clone)]
pub struct ReleaseSigning {
    /// Keys whose signatures this Operator accepts.
    pub keys: TrustedKeys,
    /// Project every signed release must name.
    pub project: String,
}

/// The newest release this Operator accepted. Persisted so a restart can't
/// be used to replay an older one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedRelease {
    /// Its sequence.
    pub sequence: u64,
    /// SHA-256 of its statement bytes.
    pub payload_sha256: String,
    /// The operator image it allows, if any.
    pub operator_image: Option<String>,
}

impl From<&VerifiedRelease> for AcceptedRelease {
    fn from(verified: &VerifiedRelease) -> Self {
        Self {
            sequence: verified.sequence,
            payload_sha256: verified.payload_sha256.clone(),
            operator_image: verified.operator_image.clone(),
        }
    }
}

/// Why a target was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// The release itself failed verification.
    #[error(transparent)]
    Release(#[from] ReleaseRejection),
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

impl ReleaseSigning {
    /// Verify `release` and return the stack to deploy: the signed stack, with
    /// image locations taken from the carried stack where the digest matches.
    /// The carried stack must then be exactly that stack, so a manager that
    /// changes anything else is refused rather than silently corrected. Sync,
    /// air-gapped bundles and the deployment loop all check releases here.
    pub fn verify_release(
        &self,
        platform: Platform,
        release: &ReleaseInfo,
    ) -> Result<VerifiedRelease, Refusal> {
        let signed = release.signed.as_ref().ok_or(ReleaseRejection::Unsigned)?;
        let verified = self.keys.verify(&self.project, platform, signed)?;
        if relocate_images(&verified.stack, &release.stack) != release.stack {
            return Err(Refusal::StackChanged);
        }
        Ok(verified)
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
        let verified = self.verify_release(platform, &target.release_info)?;
        if let Some(accepted) = accepted {
            verified.check_newer_than(accepted.sequence, &accepted.payload_sha256)?;
        }
        let config = protect_config(
            platform,
            &target.release_info.stack,
            local_input_values,
            &target.config,
        )?;
        Ok((
            TargetDeployment {
                release_info: target.release_info.clone(),
                config,
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

/// Developer inputs whose values the manager supplies, for the startup log.
pub fn manager_supplied_inputs(stack: &Stack) -> Vec<&str> {
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
    manager: &DeploymentConfig,
) -> Result<DeploymentConfig, Refusal> {
    let allowed = input_environment_names(stack);
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
    // manager. The developer declared where each one is used, so a developer
    // input can only reach the variables the signed stack maps it to.
    let input_values: HashMap<String, serde_json::Value> = stack
        .inputs
        .iter()
        .filter_map(|input| {
            let value = if deployer_supplied(input) {
                local_input_values.get(&input.id)
            } else {
                manager.input_values.get(&input.id)
            };
            value.map(|value| (input.id.clone(), value.clone()))
        })
        .collect();
    let variables = alien_core::resolve_stack_input_environment_variables(
        &stack.inputs,
        &input_values,
        platform,
    )
    .map_err(|error| Refusal::Inputs(error.message))?;

    Ok(DeploymentConfig {
        setup_run: manager.setup_run,
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::release_signature::{ReleaseSigningKey, ReleaseStatement, STATEMENT_TYPE};
    use alien_core::{
        Container, ContainerCode, EnvironmentVariable, EnvironmentVariableType, ResourceLifecycle,
        ResourceSpec, StackInputDefinition,
    };

    const DIGEST: &str = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn key() -> ReleaseSigningKey {
        ReleaseSigningKey::from_secret_bytes(&[21; 32]).unwrap()
    }

    fn signing() -> ReleaseSigning {
        ReleaseSigning {
            keys: TrustedKeys::parse(&[key().public_key()]).unwrap(),
            project: "shop".to_string(),
        }
    }

    fn input(id: &str, provider: &str, env: &str) -> StackInputDefinition {
        serde_json::from_value(serde_json::json!({
            "id": id, "kind": "string", "providedBy": [provider], "required": false,
            "label": id, "description": id,
            "env": [{ "name": env, "targetResources": ["api"] }],
        }))
        .unwrap()
    }

    fn stack(image: &str) -> Stack {
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

    fn signed_release(sequence: u64, stack: &Stack) -> ReleaseInfo {
        let signed = key()
            .sign(&ReleaseStatement {
                statement_type: STATEMENT_TYPE.to_string(),
                project: "shop".to_string(),
                platform: Platform::Kubernetes,
                sequence,
                stack: stack.clone(),
                operator_image: None,
            })
            .unwrap();
        ReleaseInfo {
            release_id: Some(format!("rel_{sequence}")),
            version: None,
            description: None,
            stack: stack.clone(),
            signed: Some(signed),
        }
    }

    fn variable(name: &str, value: &str) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_string(),
            value: value.to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        }
    }

    fn target(release_info: ReleaseInfo, variables: Vec<EnvironmentVariable>) -> TargetDeployment {
        TargetDeployment {
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

    fn local_inputs() -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_value(serde_json::json!({ "databaseUrl": "postgres://db.internal/shop" }))
            .unwrap()
    }

    fn pinned() -> String {
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
            Refusal::Release(ReleaseRejection::Unsigned)
        );
    }

    #[test]
    fn an_older_release_is_refused() {
        let accepted = AcceptedRelease {
            sequence: 6,
            payload_sha256: "other".to_string(),
            operator_image: None,
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
            Refusal::Release(ReleaseRejection::OlderSequence {
                applied: 6,
                actual: 5
            })
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

    #[test]
    fn deployer_inputs_come_from_the_installation_and_developer_inputs_from_the_manager() {
        let release = signed_release(5, &stack(&pinned()));
        let (protected, _) = signing()
            .protect_target(
                Platform::Kubernetes,
                &local_inputs(),
                None,
                &target(
                    release,
                    vec![
                        variable("LOG_LEVEL", "debug"),
                        variable("DATABASE_URL", "postgres://attacker.example/db"),
                    ],
                ),
            )
            .unwrap();
        let values: HashMap<&str, &str> = protected
            .config
            .environment_variables
            .variables
            .iter()
            .map(|variable| (variable.name.as_str(), variable.value.as_str()))
            .collect();
        assert_eq!(values["DATABASE_URL"], "postgres://db.internal/shop");
        assert_eq!(
            values["LOG_LEVEL"], "debug",
            "a developer input takes the manager's value"
        );
        assert_eq!(
            protected.config.input_values["databaseUrl"],
            serde_json::json!("postgres://db.internal/shop")
        );
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
            .verify_release(Platform::Kubernetes, &release)
            .expect("the stack as signed is deployable");

        let mut changed = release.clone();
        changed.stack = stack(&format!(
            "registry.example.com/shop/api@sha256:{}",
            "e".repeat(64)
        ));
        assert_eq!(
            signing()
                .verify_release(Platform::Kubernetes, &changed)
                .unwrap_err(),
            Refusal::StackChanged
        );
    }
}
