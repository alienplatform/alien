//! Protected mode: run only releases the developer signed.
//!
//! A chart whose stack pins signing writes the signing settings to the kept
//! ConfigMap `<release>-signing`. At start the Operator reads it itself and
//! derives the install id from the cluster (`k8s:<kube-system uid>:<ns>/<release>`);
//! nothing here comes from the manager or the Operator's own environment.
//! The install's ledger (the trust root it accepted state under, the signer
//! list version) lives in the kept
//! ConfigMap `<release>-signing-ledger`, which setup creates beside the
//! settings, and is copied to the Operator's database. At start the two
//! merge, entry by entry, to the higher; every accepted target raises both,
//! the ConfigMap first. So losing the database never lowers the signer list
//! version. Once
//! the ledger holds a trust root, settings that are missing or different
//! stop the Operator (protection is sticky).
//!
//! The manager delivers the signer list chain and the signed `release/v2`
//! statements; this Operator verifies both and decides nothing on the
//! manager's word that runs code or redirects data:
//!
//! - a target's stack comes only from its verified statement; the manager may
//!   move an image to another registry, never change its digest;
//! - a signer list older than the one accepted is refused; a release or
//!   config signed by a key no longer on the list is refused too, so after a
//!   signer is removed the current release is signed again by a current signer;
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
use std::sync::Arc;

use alien_core::release_statement::{relocate_images, signed_stack};
use alien_core::sync::TargetDeployment;
use alien_core::{
    DeletionPolicy, DeploymentConfig, EnvironmentVariable, EnvironmentVariableType,
    EnvironmentVariablesSnapshot, ExternalBindings, OtlpConfig, Platform, ReleaseInfo,
    SignedStatement, Stack, StackInputProvider, StackSettings,
};
use alien_error::{AlienError, Context};
use alien_signing::settings::names;
#[cfg(test)]
use alien_signing::Kind;
use alien_signing::{ConfigBody, InstallId, Ledger, Lookup, ReleaseBody, SigningSettings, Trust};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::ErrorData;

/// Component name of the Operator in a release's signed components.
pub const OPERATOR_COMPONENT: &str = "operator";

/// Key of the settings JSON in the `<release>-signing` ConfigMap.
pub const SETTINGS_KEY: &str = "settings";

/// What an installation pinned, and who it is.
#[derive(Debug, Clone)]
pub struct ReleaseSigning {
    /// What setup wrote.
    pub settings: SigningSettings,
    /// This install.
    pub install: InstallId,
    /// The install's ledger outside the Operator's database.
    pub ledger: Arc<dyn InstallLedger>,
}

/// What this Operator keeps between passes and restarts: the install's
/// ledger, the signer chain it verified (the ledger holds only its head),
/// and the Operator build the accepted release allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OperatorTrust {
    /// The install's ledger.
    pub ledger: Ledger,
    /// The signer list chain, oldest first.
    pub signer_lists: Vec<SignedStatement>,
    /// The Operator image digest the accepted release allows, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_digest: Option<String>,
    /// The sequence of the release applied last. Only for logging and for not re-applying the
    /// same statements; it is not a floor, and an older signed release may be applied again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_sequence: Option<u64>,
    /// The sequence of the config applied last, kept for the same reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_sequence: Option<u64>,
}

impl OperatorTrust {
    /// What an install that accepted nothing yet keeps.
    pub fn new(prefix: &str) -> Self {
        Self {
            ledger: Ledger::new(prefix),
            signer_lists: Vec::new(),
            operator_digest: None,
            release_sequence: None,
            config_sequence: None,
        }
    }
}

/// Key of the ledger JSON in ConfigMap `<release>-signing-ledger`.
pub const LEDGER_KEY: &str = "ledger";

/// The kept ConfigMap holding the install's ledger.
pub fn ledger_config_map(release: &str) -> String {
    format!("{release}-signing-ledger")
}

/// Where the install keeps its ledger outside the Operator's database.
#[async_trait]
pub trait InstallLedger: Send + Sync + std::fmt::Debug {
    /// The stored ledger JSON.
    async fn load(&self) -> crate::error::Result<Lookup>;
    /// Raise the stored ledger to `ledger`: each entry keeps the higher of
    /// the two, and a fork is an error. The ledger must already exist.
    async fn store(&self, ledger: &Ledger) -> crate::error::Result<()>;
}

/// The install ledger in a ConfigMap.
#[derive(Debug)]
pub struct ConfigMapLedger {
    /// The Kubernetes API.
    pub client: Arc<dyn alien_k8s_clients::ConfigMapApi>,
    /// The Operator's namespace.
    pub namespace: String,
    /// `<release>-signing-ledger`.
    pub name: String,
}

impl ConfigMapLedger {
    fn failed(&self, what: &str) -> ErrorData {
        ErrorData::ConfigurationError {
            message: format!("{what} ConfigMap {}/{}", self.namespace, self.name),
        }
    }
}

#[async_trait]
impl InstallLedger for ConfigMapLedger {
    async fn load(&self) -> crate::error::Result<Lookup> {
        use alien_k8s_clients::ErrorData as K8sError;
        match self
            .client
            .get_config_map(&self.namespace, &self.name)
            .await
        {
            Ok(config_map) => Ok(Lookup::Found(
                ledger_entry(&self.namespace, &self.name, config_map.data)?.into_bytes(),
            )),
            Err(error) => match error.error {
                Some(K8sError::RemoteResourceNotFound { .. }) => Ok(Lookup::Absent),
                Some(K8sError::RemoteAccessDenied { .. }) => Ok(Lookup::Denied),
                _ => Err(error).context(self.failed("reading")),
            },
        }
    }

    async fn store(&self, ledger: &Ledger) -> crate::error::Result<()> {
        let mut config_map = self
            .client
            .get_config_map(&self.namespace, &self.name)
            .await
            .context(self.failed("reading"))?;
        let stored = ledger_entry(&self.namespace, &self.name, config_map.data.clone())?;
        let mut merged = Ledger::parse(stored.as_bytes(), &ledger.prefix).context(
            ErrorData::ConfigurationError {
                message: format!(
                    "ConfigMap {}/{} does not hold this install's ledger",
                    self.namespace, self.name
                ),
            },
        )?;
        let before = merged.clone();
        merged
            .merge(ledger)
            .context(ErrorData::ConfigurationError {
                message: format!(
                    "the ledger in ConfigMap {}/{} forks from the one being written",
                    self.namespace, self.name
                ),
            })?;
        if merged == before {
            return Ok(());
        }
        config_map
            .data
            .get_or_insert_with(Default::default)
            .insert(LEDGER_KEY.to_string(), merged.to_json());
        // The resourceVersion read above makes a concurrent write a conflict.
        self.client
            .replace_config_map(&self.namespace, &self.name, &config_map)
            .await
            .context(self.failed("writing"))?;
        Ok(())
    }
}

fn ledger_entry(
    namespace: &str,
    name: &str,
    data: Option<BTreeMap<String, String>>,
) -> crate::error::Result<String> {
    data.and_then(|mut data| data.remove(LEDGER_KEY))
        .ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: format!("ConfigMap {namespace}/{name} has no '{LEDGER_KEY}' entry"),
            })
        })
}

/// Where the Operator reads its settings and its cluster's identity.
#[async_trait]
pub trait ClusterReads: Send + Sync {
    /// The settings JSON in ConfigMap `name` of `namespace`.
    async fn settings(&self, namespace: &str, name: &str) -> crate::error::Result<Lookup>;
    /// UID of the `kube-system` namespace.
    async fn cluster_uid(&self) -> crate::error::Result<String>;
}

/// Find this Operator's signing settings: the ConfigMap `<release>-signing`
/// in its own namespace, and what it keeps: the install's ledger merged
/// with `stored`, what the Operator's database holds. Missing settings mean
/// an unprotected install, unless a ledger holds a trust root (D8); a
/// ConfigMap the Operator may not read is an error, never "unprotected".
pub async fn find_signing(
    reads: &dyn ClusterReads,
    ledger: Arc<dyn InstallLedger>,
    namespace: &str,
    release: &str,
    stored: Option<&OperatorTrust>,
) -> crate::error::Result<Option<(ReleaseSigning, OperatorTrust)>> {
    let name = names::kubernetes_settings(release);
    let ledger_name = ledger_config_map(release);
    let refused = |message: String| AlienError::new(ErrorData::ConfigurationError { message });
    let settings = match reads.settings(namespace, &name).await? {
        Lookup::Found(bytes) => SigningSettings::parse(&bytes, release).context(
            ErrorData::ConfigurationError {
                message: format!("the signing settings in ConfigMap {namespace}/{name} are not valid"),
            },
        )?,
        Lookup::Absent => {
            // An unprotected chart grants no read of the ledger ConfigMap,
            // so only a ledger that reads back can say protection is gone.
            let kept = match ledger.load().await? {
                Lookup::Found(bytes) => Some(serde_json::from_slice::<Ledger>(&bytes).map_err(
                    |error| {
                        refused(format!(
                            "ConfigMap {namespace}/{ledger_name} does not hold a ledger: {error}"
                        ))
                    },
                )?),
                Lookup::Absent | Lookup::Denied => None,
            };
            let sticky = kept
                .iter()
                .chain(stored.map(|stored| &stored.ledger))
                .find(|ledger| ledger.is_sticky());
            if let Some(sticky) = sticky {
                return Err(refused(format!(
                    "ConfigMap {namespace}/{name} with the signing settings is gone, and this install accepted signed state under fingerprint {}; nothing new runs until an administrator restores it",
                    sticky.fingerprint.as_deref().unwrap_or_default()
                )));
            }
            return Ok(None);
        }
        Lookup::Denied => {
            return Err(refused(format!(
                "the Operator may not read ConfigMap {namespace}/{name}, so it can't tell whether this install is protected"
            )))
        }
    };
    let mut merged = match ledger.load().await? {
        Lookup::Found(bytes) => Ledger::parse(&bytes, &settings.prefix).context(
            ErrorData::ConfigurationError {
                message: format!("ConfigMap {namespace}/{ledger_name} does not hold this install's ledger"),
            },
        )?,
        Lookup::Absent => {
            return Err(refused(format!(
                "ConfigMap {namespace}/{ledger_name} with the install's ledger is missing; setup creates it with the signing settings, and the Operator never starts a protected install from an empty ledger"
            )))
        }
        Lookup::Denied => {
            return Err(refused(format!(
                "the Operator may not read ConfigMap {namespace}/{ledger_name} with the install's ledger"
            )))
        }
    };
    if let Some(stored) = stored {
        merged
            .merge(&stored.ledger)
            .context(ErrorData::ConfigurationError {
                message: "the Operator's database holds a ledger that forks from the install's"
                    .to_string(),
            })?;
    }
    let install = InstallId::Kubernetes {
        cluster: reads.cluster_uid().await?,
        namespace: namespace.to_string(),
        release: release.to_string(),
    };
    merged
        .check_root(Some(&settings.fingerprint), &install)
        .context(ErrorData::ConfigurationError {
            message: "the signing settings differ from the ones this install accepted state under"
                .to_string(),
        })?;
    let kept = OperatorTrust {
        ledger: merged,
        signer_lists: stored
            .map(|stored| stored.signer_lists.clone())
            .unwrap_or_default(),
        operator_digest: stored.and_then(|stored| stored.operator_digest.clone()),
        release_sequence: stored.and_then(|stored| stored.release_sequence),
        config_sequence: stored.and_then(|stored| stored.config_sequence),
    };
    Ok(Some((
        ReleaseSigning {
            settings,
            install,
            ledger,
        },
        kept,
    )))
}

/// The cluster reads through the Kubernetes API.
pub struct KubernetesReads<'a>(pub &'a dyn alien_k8s_clients::ConfigMapApi);

#[async_trait]
impl ClusterReads for KubernetesReads<'_> {
    async fn settings(&self, namespace: &str, name: &str) -> crate::error::Result<Lookup> {
        use alien_k8s_clients::ErrorData as K8sError;
        match self.0.get_config_map(namespace, name).await {
            Ok(config_map) => {
                let value = config_map
                    .data
                    .and_then(|mut data| data.remove(SETTINGS_KEY))
                    .ok_or_else(|| {
                        AlienError::new(ErrorData::ConfigurationError {
                            message: format!(
                                "ConfigMap {namespace}/{name} has no '{SETTINGS_KEY}' entry"
                            ),
                        })
                    })?;
                Ok(Lookup::Found(value.into_bytes()))
            }
            Err(error) => match error.error {
                Some(K8sError::RemoteResourceNotFound { .. }) => Ok(Lookup::Absent),
                Some(K8sError::RemoteAccessDenied { .. }) => Ok(Lookup::Denied),
                _ => Err(error).context(ErrorData::ConfigurationError {
                    message: format!("reading ConfigMap {namespace}/{name}"),
                }),
            },
        }
    }

    async fn cluster_uid(&self) -> crate::error::Result<String> {
        let namespace =
            self.0
                .get_namespace("kube-system")
                .await
                .context(ErrorData::ConfigurationError {
                    message: "reading the kube-system namespace for the install id".to_string(),
                })?;
        namespace
            .metadata
            .uid
            .filter(|uid| !uid.is_empty())
            .ok_or_else(|| {
                AlienError::new(ErrorData::ConfigurationError {
                    message: "the kube-system namespace has no UID".to_string(),
                })
            })
    }
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
    /// Signers whose signatures counted; empty for the release the install
    /// already accepted, which is kept rather than verified again.
    pub signers: Vec<String>,
    /// The verified release body.
    pub body: ReleaseBody,
    /// The trust it verified under.
    pub trust: Trust,
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
    /// A binding the signed config sets doesn't fit the signed stack.
    #[error("the signed config's binding '{id}' is refused: {reason}")]
    Binding {
        /// Resource id.
        id: String,
        /// Why.
        reason: String,
    },
    /// The carried stack differs from the one its statement signs.
    #[error("the stack the manager sent differs from the signed release")]
    StackChanged,
    /// The manager asked to replace volumes with snapshots. A snapshot holds
    /// whatever the manager chose, so a protected installation takes none.
    #[error(
        "the manager asked to restore volumes ({}); a protected installation takes no volume restores from its manager",
        .0.join(", ")
    )]
    VolumeRestores(Vec<String>),
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
    /// manager can deliver newer list versions but never an older one. The
    /// release must verify under the current list: one whose signer has left
    /// the list is refused until a current signer signs it.
    pub fn verify_release(
        &self,
        platform: Platform,
        release: &ReleaseInfo,
        stored: &OperatorTrust,
    ) -> Result<VerifiedRelease, Refusal> {
        let signed = release.signed.as_ref().ok_or(Refusal::Unsigned)?;
        let chain = if stored.signer_lists.is_empty() {
            &release.signer_lists
        } else {
            &stored.signer_lists
        };
        let now = Utc::now();
        let mut trust = Trust::new(
            self.settings.clone(),
            self.install.clone(),
            chain,
            &stored.ledger,
            now,
        )?;
        trust.extend_chain(&release.signer_lists, now)?;
        let verified = trust.verify_release(signed, now)?;
        let (sequence, digest, body, signers) = (
            verified.sequence,
            verified.digest,
            verified.body,
            verified.signers.into_iter().collect::<Vec<_>>(),
        );
        let stack = signed_stack(&body, platform)?;
        if relocate_images(&stack, &release.stack) != release.stack {
            return Err(Refusal::StackChanged);
        }
        Ok(VerifiedRelease {
            sequence,
            digest,
            stack,
            signers,
            body,
            trust,
        })
    }

    /// The target to store for `target`, the release it verified, and what
    /// the Operator keeps after accepting it. Refuses a release or config
    /// the current signer list does not vouch for.
    pub fn protect_target(
        &self,
        platform: Platform,
        local_input_values: &serde_json::Map<String, serde_json::Value>,
        stored: &OperatorTrust,
        target: &TargetDeployment,
    ) -> Result<(TargetDeployment, VerifiedRelease, OperatorTrust), Refusal> {
        let verified = self.verify_release(platform, &target.release_info, stored)?;
        let mut ledger = stored.ledger.clone();
        let (signed_config, config_sequence) = match &target.signed_config {
            None => (ConfigBody::default(), None),
            Some(signed) => {
                let config = verified
                    .trust
                    .verify_config(signed, &verified.body, Utc::now())?;
                (config.body, Some(config.sequence))
            }
        };
        verified.trust.record(&mut ledger)?;
        let kept = OperatorTrust {
            ledger,
            signer_lists: verified.trust.chain().statements().to_vec(),
            operator_digest: verified.body.components.get(OPERATOR_COMPONENT).cloned(),
            release_sequence: Some(verified.sequence),
            config_sequence,
        };
        let config = protect_config(
            platform,
            &target.release_info.stack,
            &verified.body.configurable.env,
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
            kept,
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

/// The bindings of resources outside this cluster, from the signed config.
/// Each names a resource of the signed stack and is read as that resource's
/// type of binding.
fn signed_external_bindings(
    stack: &Stack,
    signed_config: &ConfigBody,
) -> Result<ExternalBindings, Refusal> {
    let mut bindings = ExternalBindings::default();
    for (id, value) in &signed_config.bindings {
        let refuse = |reason: String| Refusal::Binding {
            id: id.clone(),
            reason,
        };
        let entry = stack
            .resources()
            .find(|(resource, _)| resource.as_str() == id)
            .map(|(_, entry)| entry)
            .ok_or_else(|| refuse("the signed stack has no such resource".to_string()))?;
        let mut typed = value
            .as_object()
            .cloned()
            .ok_or_else(|| refuse("its value is not an object".to_string()))?;
        let resource_type = entry.config.resource_type().to_string().replace('-', "_");
        typed.insert("type".to_string(), serde_json::Value::String(resource_type));
        let binding = serde_json::from_value(serde_json::Value::Object(typed))
            .map_err(|error| refuse(format!("it is not a binding of this resource: {error}")))?;
        bindings.insert(id.as_str(), binding);
    }
    Ok(bindings)
}

/// OTLP export to `endpoint`, the signed config's OTLP/HTTP base URL, which
/// the signing crate already limited to `https://`.
fn signed_telemetry(endpoint: &str) -> OtlpConfig {
    let base = endpoint.trim_end_matches('/');
    OtlpConfig {
        logs_endpoint: format!("{base}/v1/logs"),
        logs_auth_header: String::new(),
        metrics_endpoint: Some(format!("{base}/v1/metrics")),
        metrics_auth_header: None,
        resource_attributes: HashMap::new(),
    }
}

/// Rebuild the manager's configuration from what this installation and the
/// signed stack allow. Every field is decided here, so a field added later must
/// be decided too.
fn protect_config(
    platform: Platform,
    stack: &Stack,
    configurable_env: &[String],
    local_input_values: &serde_json::Map<String, serde_json::Value>,
    signed_config: &ConfigBody,
    manager: &DeploymentConfig,
) -> Result<DeploymentConfig, Refusal> {
    if !manager.volume_restores.is_empty() {
        return Err(Refusal::VolumeRestores(
            manager
                .volume_restores
                .iter()
                .map(|restore| restore.request_id.clone())
                .collect(),
        ));
    }
    // Names the release lets a signed config set may arrive too: the
    // configuration applied under an older config still carries them. Their
    // values are rebuilt below from the signed config, which may drop them.
    let mut allowed = input_environment_names(stack);
    allowed.extend(configurable_env.iter().map(String::as_str));
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
    let deployer_values: HashMap<String, serde_json::Value> = input_values
        .iter()
        .filter(|(id, _)| {
            stack
                .inputs
                .iter()
                .any(|input| input.id == **id && deployer_supplied(input))
        })
        .map(|(id, value)| (id.clone(), value.clone()))
        .collect();
    // Developer inputs resolve the way the cloud controllers and verifiers resolve them.
    let mut variables = alien_core::release_statement::signed_input_variables(
        stack,
        platform,
        &signed_config.inputs,
        &deployer_values,
    )
    .map_err(|error| Refusal::Inputs(error.message))?;
    // Deployer inputs have their own mappings, which this installation's values fill.
    let deployer_inputs: Vec<alien_core::StackInputDefinition> = stack
        .inputs
        .iter()
        .map(|input| {
            let mut input = input.clone();
            if !deployer_supplied(&input) {
                input.env.clear();
            }
            input
        })
        .collect();
    let deployer_variables = alien_core::resolve_stack_input_environment_variables(
        &deployer_inputs,
        &input_values,
        platform,
    )
    .map_err(|error| Refusal::Inputs(error.message))?;
    if let Some(name) = deployer_variables
        .iter()
        .find(|deployer| {
            variables
                .iter()
                .any(|developer| developer.name == deployer.name)
        })
        .map(|variable| variable.name.clone())
    {
        return Err(Refusal::Inputs(format!(
            "Stack inputs map more than one value to environment variable '{name}'"
        )));
    }
    variables.extend(deployer_variables);
    // Configurable env the signed config sets, which the release already
    // limited to names it lists.
    let configured: BTreeMap<&String, &String> = signed_config.env.iter().collect();
    variables.retain(|variable| !configured.contains_key(&variable.name));
    variables.extend(
        configured
            .into_iter()
            .map(|(name, value)| EnvironmentVariable {
                name: name.clone(),
                value: value.clone(),
                var_type: EnvironmentVariableType::Plain,
                target_resources: None,
            }),
    );

    Ok(DeploymentConfig {
        setup_run: manager.setup_run,
        setup_run_id: manager.setup_run_id.clone(),
        environment_variables: EnvironmentVariablesSnapshot {
            variables,
            hash: manager.environment_variables.hash.clone(),
            created_at: manager.environment_variables.created_at.clone(),
        },
        input_values,
        // Values the manager holds never reach a protected install: its
        // variables and inputs come from the signed config above.
        value_slots: Vec::new(),
        // Secret inputs are resolved from the deployment's own vault, and
        // what the manager lists as stored changes nothing in the signed list.
        stored_secret_input_ids: Some(Vec::new()),
        // Logs can carry customer data, so they go only where the signed
        // config says, with no headers the manager chose.
        monitoring: signed_config
            .telemetry_endpoint
            .as_deref()
            .map(signed_telemetry),
        volume_restores: Vec::new(),
        // Timing and pull credentials: an outage at worst.
        allow_frozen_changes: manager.allow_frozen_changes,
        manager_url: manager.manager_url.clone(),
        deployment_token: manager.deployment_token.clone(),
        // A manager's delete keeps the install's data; erasing it is the
        // customer's own act on their cluster.
        deletion_policy: DeletionPolicy::Retain,
        // Where data goes and what the deployment is called come from this
        // installation; the deployment loop fills these from its settings.
        deployment_name: None,
        stack_settings: StackSettings::default(),
        // Resources outside this cluster bind only to values the signed
        // config names; the manager's are ignored.
        external_bindings: signed_external_bindings(stack, signed_config)?,
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
        signed_config: None,
    })
}

/// The release key of the test project.
#[cfg(test)]
pub(crate) fn test_key(seed: u8) -> alien_signing::SecretKey {
    let mut secret = [seed; 32];
    secret[0] = 1;
    alien_signing::SecretKey::from_secret_bytes(&secret).unwrap()
}

/// One `notAfter` for every test signer list in this process, so building
/// the same list twice gives the same digest.
#[cfg(test)]
fn test_not_after() -> chrono::DateTime<Utc> {
    static NOT_AFTER: std::sync::OnceLock<chrono::DateTime<Utc>> = std::sync::OnceLock::new();
    *NOT_AFTER.get_or_init(|| Utc::now() + chrono::Duration::days(60))
}

/// A statement of `kind` for `project`, unsigned.
#[cfg(test)]
pub(crate) fn test_statement<B: Serialize>(
    project: &str,
    kind: Kind,
    install: Option<&str>,
    sequence: u64,
    signer_list_version: u64,
    body: B,
) -> SignedStatement {
    let envelope = alien_signing::envelope::Envelope {
        kind: kind.as_str().to_string(),
        project: project.to_string(),
        install: install.map(str::to_string),
        sequence,
        not_after: (kind == Kind::SignerList).then(test_not_after),
        signer_list_version,
        body,
    };
    SignedStatement::new(&envelope.to_payload().unwrap())
}

/// Version 1 of a signer list for `project`: CI may release, a device may
/// release and administer the list.
#[cfg(test)]
pub(crate) fn test_signer_list(project: &str) -> SignedStatement {
    test_statement(
        project,
        Kind::SignerList,
        None,
        1,
        1,
        test_list_body(None, true),
    )
    .sign_with("alice-laptop", &test_key(2))
    .unwrap()
}

#[cfg(test)]
fn test_list_body(previous: Option<String>, with_ci: bool) -> alien_signing::SignerListBody {
    use alien_signing::{Role, Signer, SignerKind, SignerListBody};
    let signer = |id: &str, seed: u8, kind: SignerKind, roles: Vec<Role>| Signer {
        id: id.to_string(),
        key: test_key(seed).public_key().as_str().to_string(),
        kind,
        roles,
        label: None,
    };
    let mut signers = vec![signer(
        "alice-laptop",
        2,
        SignerKind::Device,
        vec![Role::Release, Role::Admin],
    )];
    if with_ci {
        signers.push(signer("ci", 4, SignerKind::Ci, vec![Role::Release]));
    }
    SignerListBody {
        previous,
        admin_threshold: 1,
        signers,
        deny: Vec::new(),
        recovered: false,
    }
}

/// Version 2 of the test signer list, which removes the CI key.
#[cfg(test)]
pub(crate) fn test_signer_list_without_ci(project: &str) -> SignedStatement {
    let previous = test_signer_list(project).open().unwrap().digest;
    test_statement(
        project,
        Kind::SignerList,
        None,
        2,
        1,
        test_list_body(Some(previous), false),
    )
    .sign_with("alice-laptop", &test_key(2))
    .unwrap()
}

/// A protected configuration for tests: settings pinning the test signer
/// list of `project`, for release `project` in namespace `apps`.
#[cfg(test)]
pub(crate) fn test_signing(project: &str) -> ReleaseSigning {
    ReleaseSigning {
        settings: SigningSettings {
            fingerprint: alien_signing::fingerprint(
                &test_signer_list(project).open().unwrap().digest,
            ),
            prefix: project.to_string(),
            channel: "production".to_string(),
        },
        install: InstallId::Kubernetes {
            cluster: "8a2f0c4e-1111-2222-3333-444455556666".to_string(),
            namespace: "apps".to_string(),
            release: project.to_string(),
        },
        ledger: Arc::new(MemoryLedger::created(project)),
    }
}

/// An install ledger held in memory, as setup creates it or absent.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct MemoryLedger(pub(crate) std::sync::Mutex<Option<Ledger>>);

#[cfg(test)]
impl MemoryLedger {
    /// The ledger setup creates for `prefix`.
    pub(crate) fn created(prefix: &str) -> Self {
        Self(std::sync::Mutex::new(Some(Ledger::new(prefix))))
    }

    /// What it holds now.
    pub(crate) fn held(&self) -> Option<Ledger> {
        self.0.lock().unwrap().clone()
    }
}

#[cfg(test)]
#[async_trait]
impl InstallLedger for MemoryLedger {
    async fn load(&self) -> crate::error::Result<Lookup> {
        Ok(match self.held() {
            Some(ledger) => Lookup::Found(ledger.to_json().into_bytes()),
            None => Lookup::Absent,
        })
    }

    async fn store(&self, ledger: &Ledger) -> crate::error::Result<()> {
        let mut held = self.0.lock().unwrap();
        let stored = held.as_mut().ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "the install ledger does not exist".to_string(),
            })
        })?;
        stored.merge(ledger).context(ErrorData::ConfigurationError {
            message: "the install ledger forks".to_string(),
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use alien_core::release_statement::{release_body, ReleaseOptions};
    use alien_core::{
        Container, ContainerCode, EnvironmentVariable, EnvironmentVariableType, ResourceLifecycle,
        ResourceSpec, StackInputDefinition,
    };
    use std::sync::Mutex;

    const DIGEST: &str = "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    pub(crate) fn signer_list() -> SignedStatement {
        test_signer_list("shop")
    }

    pub(crate) fn signing() -> ReleaseSigning {
        test_signing("shop")
    }

    fn fresh() -> OperatorTrust {
        OperatorTrust::new("shop")
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
        build_stack(image, false)
    }

    /// The test stack with a storage resource `data` beside the container.
    fn stack_with_storage(image: &str) -> Stack {
        build_stack(image, true)
    }

    fn build_stack(image: &str, storage: bool) -> Stack {
        let data = alien_core::Storage::new("data".to_string()).build();
        let mut container = Container::new("api".to_string());
        if storage {
            container = container.link(&data);
        }
        let container = container
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
        let mut builder = Stack::new("shop".to_string()).add(container, ResourceLifecycle::Live);
        if storage {
            builder = builder.add(data, ResourceLifecycle::Frozen);
        }
        let mut stack = builder.build();
        stack.inputs = vec![
            input("logLevel", "developer", "LOG_LEVEL"),
            input("databaseUrl", "deployer", "DATABASE_URL"),
        ];
        stack
    }

    /// A release of `stack` on `channel`, signed by `signer` (`ci`: seed 4,
    /// `alice-laptop`: seed 2).
    pub(crate) fn release_by(
        sequence: u64,
        stack: &Stack,
        channel: &str,
        signer: &str,
    ) -> ReleaseInfo {
        release_with(
            sequence,
            stack,
            ReleaseOptions {
                channel: channel.to_string(),
                ..ReleaseOptions::default()
            },
            signer,
        )
    }

    /// A release of `stack` built with `options`, signed by `signer`.
    pub(crate) fn release_with(
        sequence: u64,
        stack: &Stack,
        options: ReleaseOptions,
        signer: &str,
    ) -> ReleaseInfo {
        let seed = if signer == "ci" { 4 } else { 2 };
        let body = release_body(Platform::Kubernetes, stack, &options).unwrap();
        let signed = test_statement("shop", Kind::Release, None, sequence, 1, body)
            .sign_with(signer, &test_key(seed))
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

    pub(crate) fn signed_release(sequence: u64, stack: &Stack) -> ReleaseInfo {
        release_by(sequence, stack, "production", "ci")
    }

    pub(crate) fn variable(name: &str, value: &str) -> EnvironmentVariable {
        EnvironmentVariable {
            name: name.to_string(),
            value: value.to_string(),
            var_type: EnvironmentVariableType::Plain,
            target_resources: None,
        }
    }

    pub(crate) fn target(
        release_info: ReleaseInfo,
        variables: Vec<EnvironmentVariable>,
    ) -> TargetDeployment {
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

    fn accept(stored: &OperatorTrust, target: &TargetDeployment) -> Result<OperatorTrust, Refusal> {
        signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), stored, target)
            .map(|(_, _, kept)| kept)
    }

    #[test]
    fn a_stack_other_than_the_signed_one_is_refused() {
        let signed = stack(&pinned());
        let release = signed_release(5, &signed);
        let (protected, verified, _) = signing()
            .protect_target(
                Platform::Kubernetes,
                &local_inputs(),
                &fresh(),
                &target(release.clone(), vec![]),
            )
            .unwrap();
        assert_eq!(protected.release_info.stack, signed);
        assert_eq!(verified.sequence, 5);

        let mut moved = release.clone();
        moved.stack = stack(&format!("mirror.example.com/shop/api@{DIGEST}"));
        accept(&fresh(), &target(moved, vec![]))
            .expect("an image may move to another registry under the same digest");

        let mut changed = release;
        changed.stack = stack(&format!(
            "registry.example.com/shop/api@sha256:{}",
            "e".repeat(64)
        ));
        assert_eq!(
            accept(&fresh(), &target(changed, vec![])).unwrap_err(),
            Refusal::StackChanged
        );
    }

    #[test]
    fn an_unsigned_target_is_refused() {
        let mut release = signed_release(5, &stack(&pinned()));
        release.signed = None;
        assert_eq!(
            accept(&fresh(), &target(release, vec![])).unwrap_err(),
            Refusal::Unsigned
        );
    }

    #[test]
    fn the_ledger_records_the_trust_root_and_an_older_signed_release_runs_again() {
        let stored = accept(
            &fresh(),
            &target(signed_release(6, &stack(&pinned())), vec![]),
        )
        .unwrap();
        assert_eq!(stored.release_sequence, Some(6));
        assert!(stored.ledger.is_sticky());

        // The ledger holds trust, not a version floor: the same release passes again, and so
        // does an older one signed by a current signer.
        accept(
            &stored,
            &target(signed_release(6, &stack(&pinned())), vec![]),
        )
        .expect("the accepted release passes again");
        let older = accept(
            &stored,
            &target(signed_release(5, &stack(&pinned())), vec![]),
        )
        .expect("an older signed release may run");
        assert_eq!(older.release_sequence, Some(5));
    }

    #[test]
    fn a_release_for_another_channel_is_refused() {
        let refusal = accept(
            &fresh(),
            &target(release_by(5, &stack(&pinned()), "dev", "ci"), vec![]),
        )
        .unwrap_err();
        assert!(
            matches!(&refusal, Refusal::Release { message, .. } if message.contains("dev")),
            "{refusal}"
        );
    }

    /// Removing a signer withdraws the vendor's word for everything that key signed, the release
    /// the install runs included. The release is signed again by a current signer.
    #[test]
    fn removing_the_ci_key_refuses_what_it_signed_until_a_current_signer_signs_it() {
        let accepted_release = signed_release(5, &stack(&pinned()));
        let stored = accept(&fresh(), &target(accepted_release.clone(), vec![])).unwrap();

        let mut without_ci = accepted_release.clone();
        without_ci.signer_lists = vec![signer_list(), test_signer_list_without_ci("shop")];

        let refusal = signing()
            .verify_release(Platform::Kubernetes, &without_ci, &stored)
            .err()
            .expect("the CI key is gone from the list")
            .to_string();
        assert!(
            refusal.contains("not on signer list version 2"),
            "{refusal}"
        );

        // The same release signed by a key still on the list lands.
        let mut by_device = release_by(5, &stack(&pinned()), "production", "alice-laptop");
        by_device.signer_lists = without_ci.signer_lists.clone();
        let stored =
            accept(&stored, &target(by_device, vec![])).expect("a current signer's release");
        assert_eq!(stored.ledger.signer_list.as_ref().unwrap().sequence, 2);

        // A config the removed CI key signed is refused too.
        let mut next_release = release_by(6, &stack(&pinned()), "production", "alice-laptop");
        next_release.signer_lists = without_ci.signer_lists.clone();
        let mut new_config = target(next_release, vec![]);
        new_config.signed_config = Some(signed_config(
            2,
            &this_install(),
            serde_json::json!({"inputs": {"logLevel": "trace"}}),
        ));
        let refusal = accept(&stored, &new_config).unwrap_err().to_string();
        assert!(
            refusal.contains("not on signer list version 2"),
            "{refusal}"
        );
    }

    #[test]
    fn manager_environment_variables_are_refused_by_name() {
        let release = signed_release(5, &stack(&pinned()));
        let refusal = accept(
            &fresh(),
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

    /// A `config/v1` for `install`, signed by the CI key.
    pub(crate) fn signed_config(
        sequence: u64,
        install: &str,
        body: serde_json::Value,
    ) -> SignedStatement {
        test_statement("shop", Kind::Config, Some(install), sequence, 1, body)
            .sign_with("ci", &test_key(4))
            .unwrap()
    }

    pub(crate) fn this_install() -> String {
        signing().install.to_string()
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
        let (protected, _, _) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), &fresh(), &target)
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
            &this_install(),
            serde_json::json!({"inputs": {"logLevel": "warn"}}),
        ));
        let (protected, _, stored) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), &fresh(), &target)
            .unwrap();
        assert_eq!(
            values(&protected)["LOG_LEVEL"],
            "warn",
            "the signed value runs"
        );
        assert_eq!(stored.config_sequence, Some(3));

        // Config signed for another install, or setting what the release
        // doesn't make configurable, is refused.
        target.signed_config = Some(signed_config(
            4,
            "k8s:8a2f0c4e-1111-2222-3333-444455556666:other/shop",
            serde_json::json!({"inputs": {"logLevel": "warn"}}),
        ));
        assert!(accept(&fresh(), &target).is_err());
        target.signed_config = Some(signed_config(
            4,
            &this_install(),
            serde_json::json!({"inputs": {"databaseUrl": "postgres://attacker.example/db"}}),
        ));
        assert!(accept(&fresh(), &target).is_err());
    }

    fn gated_stack() -> Stack {
        let mut stack = stack(&pinned());
        stack.inputs.extend(
            serde_json::from_value::<Vec<StackInputDefinition>>(serde_json::json!([
                {"id": "audit", "kind": "boolean", "providedBy": ["deployer"], "required": false,
                 "label": "audit", "description": "audit"},
                {"id": "auditSink", "kind": "string", "providedBy": ["developer"], "required": false,
                 "label": "sink", "description": "sink",
                 "when": {"input": "audit", "equals": true},
                 "env": [{"name": "AUDIT_SINK", "targetResources": ["api"]}]},
            ]))
            .unwrap(),
        );
        stack
    }

    fn with_audit(audit: bool) -> serde_json::Map<String, serde_json::Value> {
        let mut inputs = local_inputs();
        inputs.insert("audit".to_string(), serde_json::json!(audit));
        inputs
    }

    #[test]
    fn a_signed_developer_input_follows_the_installations_own_condition_values() {
        let stack = gated_stack();
        let mut target = target(signed_release(5, &stack), vec![]);
        target.signed_config = Some(signed_config(
            3,
            &this_install(),
            serde_json::json!({"inputs": {"auditSink": "https://audit.example/hook"}}),
        ));
        let run = |inputs| {
            signing()
                .protect_target(Platform::Kubernetes, &inputs, &fresh(), &target)
                .map(|(protected, _, _)| values(&protected))
        };
        assert_eq!(
            run(with_audit(true)).unwrap()["AUDIT_SINK"],
            "https://audit.example/hook",
            "the signed value runs where the installation switched its condition on"
        );
        assert!(
            !run(with_audit(false)).unwrap().contains_key("AUDIT_SINK"),
            "and not where it is off"
        );
    }

    #[test]
    fn two_inputs_mapping_one_env_name_are_refused() {
        let mut stack = stack(&pinned());
        stack
            .inputs
            .push(input("otherLevel", "deployer", "LOG_LEVEL"));
        let mut target = target(signed_release(5, &stack), vec![]);
        target.signed_config = Some(signed_config(
            3,
            &this_install(),
            serde_json::json!({"inputs": {"logLevel": "warn"}}),
        ));
        let mut inputs = local_inputs();
        inputs.insert("otherLevel".to_string(), serde_json::json!("trace"));
        let refusal = signing()
            .protect_target(Platform::Kubernetes, &inputs, &fresh(), &target)
            .unwrap_err();
        assert!(
            matches!(&refusal, Refusal::Inputs(message) if message.contains("LOG_LEVEL")),
            "{refusal}"
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

        let (protected, _, _) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), &fresh(), &target)
            .unwrap();
        assert_eq!(protected.config.native_image_host, None);
        assert_eq!(protected.config.observe_label_selector, None);
        assert!(protected.config.external_bindings.0.is_empty());
    }

    #[test]
    fn the_manager_cannot_choose_erase_or_stored_secret_inputs() {
        let release = signed_release(5, &stack(&pinned()));
        let mut target = target(release, vec![]);
        target.config.deletion_policy = alien_core::DeletionPolicy::Erase;
        target.config.stored_secret_input_ids = Some(vec!["databasePassword".to_string()]);

        let (protected, _, _) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), &fresh(), &target)
            .unwrap();
        assert_eq!(
            protected.config.deletion_policy,
            alien_core::DeletionPolicy::Retain
        );
        assert_eq!(protected.config.stored_secret_input_ids, Some(Vec::new()));
    }

    #[test]
    fn external_bindings_come_only_from_signed_config() {
        let release = signed_release(5, &stack_with_storage(&pinned()));
        let mut target = target(release, vec![]);
        // Attack: the manager points the binding at its own bucket.
        target.config.external_bindings.insert(
            "data",
            alien_core::ExternalBinding::Storage(alien_core::StorageBinding::s3("attacker-bucket")),
        );
        let (protected, _, _) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), &fresh(), &target)
            .unwrap();
        assert!(
            protected.config.external_bindings.0.is_empty(),
            "the manager's binding is ignored"
        );

        target.signed_config = Some(signed_config(
            1,
            &this_install(),
            serde_json::json!({"bindings": {"data": {"service": "s3", "bucketName": "shop-data"}}}),
        ));
        let (protected, _, _) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), &fresh(), &target)
            .unwrap();
        assert_eq!(
            protected.config.external_bindings.0,
            HashMap::from([(
                "data".to_string(),
                alien_core::ExternalBinding::Storage(alien_core::StorageBinding::s3("shop-data"))
            )])
        );

        for (bindings, reason) in [
            (
                serde_json::json!({"cache": {"service": "s3", "bucketName": "x"}}),
                "SIGNING_BODY_REFUSED: The 'config/v1' statement was refused: binding 'cache' is not a binding of the signed release",
            ),
            (
                serde_json::json!({"data": {"service": "redis", "url": "redis://x"}}),
                "the signed config's binding 'data' is refused: it is not a binding of this resource",
            ),
        ] {
            target.signed_config = Some(signed_config(
                2,
                &this_install(),
                serde_json::json!({ "bindings": bindings }),
            ));
            let refusal = accept(&fresh(), &target).unwrap_err().to_string();
            assert!(refusal.starts_with(reason), "{refusal}");
        }
    }

    #[test]
    fn telemetry_goes_only_where_the_signed_config_says() {
        let release = signed_release(5, &stack(&pinned()));
        let mut target = target(release, vec![]);
        // Attack: the manager points logs at its own collector.
        target.config.monitoring = Some(OtlpConfig {
            logs_endpoint: "https://collector.attacker.example/v1/logs".to_string(),
            logs_auth_header: "authorization=Bearer attacker".to_string(),
            metrics_endpoint: None,
            metrics_auth_header: None,
            resource_attributes: HashMap::new(),
        });
        let (protected, _, _) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), &fresh(), &target)
            .unwrap();
        assert_eq!(
            protected.config.monitoring, None,
            "no signed endpoint, no export"
        );

        target.signed_config = Some(signed_config(
            1,
            &this_install(),
            serde_json::json!({"telemetryEndpoint": "https://otel.shop.example/"}),
        ));
        let (protected, _, _) = signing()
            .protect_target(Platform::Kubernetes, &local_inputs(), &fresh(), &target)
            .unwrap();
        assert_eq!(
            protected.config.monitoring,
            Some(OtlpConfig {
                logs_endpoint: "https://otel.shop.example/v1/logs".to_string(),
                logs_auth_header: String::new(),
                metrics_endpoint: Some("https://otel.shop.example/v1/metrics".to_string()),
                metrics_auth_header: None,
                resource_attributes: HashMap::new(),
            })
        );
    }

    #[test]
    fn volume_restores_from_the_manager_are_refused() {
        let release = signed_release(5, &stack(&pinned()));
        let mut target = target(release, vec![]);
        // Off: without restores the target is accepted.
        accept(&fresh(), &target).expect("no restores, accepted");
        // Attack: replace the database volume with a snapshot the manager chose.
        target.config.volume_restores = vec![alien_core::VolumeRestoreRequest {
            request_id: "vr_1".to_string(),
            resource_id: "api".to_string(),
            ordinal: 0,
            snapshot_id: "snap-0123456789abcdef0".to_string(),
        }];
        let refusal = accept(&fresh(), &target).unwrap_err();
        assert_eq!(refusal, Refusal::VolumeRestores(vec!["vr_1".to_string()]));
        assert_eq!(
            refusal.to_string(),
            "the manager asked to restore volumes (vr_1); a protected installation takes no volume restores from its manager"
        );
    }

    #[test]
    fn a_stack_written_after_verification_is_caught_before_deploying() {
        let signed = stack(&pinned());
        let release = signed_release(5, &signed);
        signing()
            .verify_release(Platform::Kubernetes, &release, &fresh())
            .expect("the stack as signed is deployable");

        let mut changed = release.clone();
        changed.stack = stack(&format!(
            "registry.example.com/shop/api@sha256:{}",
            "e".repeat(64)
        ));
        assert_eq!(
            signing()
                .verify_release(Platform::Kubernetes, &changed, &fresh())
                .unwrap_err(),
            Refusal::StackChanged
        );
    }

    /// A cluster with a settings ConfigMap, or none, or one the Operator may
    /// not read.
    pub(crate) struct Cluster {
        settings: Mutex<Lookup>,
        uid: String,
    }

    #[async_trait]
    impl ClusterReads for Cluster {
        async fn settings(&self, namespace: &str, name: &str) -> crate::error::Result<Lookup> {
            assert_eq!((namespace, name), ("apps", "shop-signing"));
            Ok(match &*self.settings.lock().unwrap() {
                Lookup::Found(bytes) => Lookup::Found(bytes.clone()),
                Lookup::Absent => Lookup::Absent,
                Lookup::Denied => Lookup::Denied,
            })
        }
        async fn cluster_uid(&self) -> crate::error::Result<String> {
            Ok(self.uid.clone())
        }
    }

    fn cluster(settings: Lookup) -> Cluster {
        Cluster {
            settings: Mutex::new(settings),
            uid: "8a2f0c4e-1111-2222-3333-444455556666".to_string(),
        }
    }

    /// A cluster holding the test settings.
    pub(crate) fn cluster_with_settings() -> Cluster {
        cluster(settings_bytes())
    }

    fn settings_bytes() -> Lookup {
        Lookup::Found(signing().settings.to_json().into_bytes())
    }

    async fn find(
        cluster: &Cluster,
        stored: Option<&OperatorTrust>,
    ) -> crate::error::Result<Option<ReleaseSigning>> {
        find_with(cluster, Arc::new(MemoryLedger::created("shop")), stored)
            .await
            .map(|found| found.map(|(signing, _)| signing))
    }

    async fn find_with(
        cluster: &Cluster,
        ledger: Arc<MemoryLedger>,
        stored: Option<&OperatorTrust>,
    ) -> crate::error::Result<Option<(ReleaseSigning, OperatorTrust)>> {
        find_signing(cluster, ledger, "apps", "shop", stored).await
    }

    #[tokio::test]
    async fn the_settings_and_install_id_come_from_the_cluster() {
        let found = find(&cluster(settings_bytes()), None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.settings, signing().settings);
        assert_eq!(found.install, signing().install);
        assert!(find(&cluster(Lookup::Absent), None)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn a_protected_install_never_starts_from_an_empty_ledger() {
        let error = find_with(
            &cluster(settings_bytes()),
            Arc::new(MemoryLedger(Mutex::new(None))),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.message,
            "ConfigMap apps/shop-signing-ledger with the install's ledger is missing; setup creates it with the signing settings, and the Operator never starts a protected install from an empty ledger"
        );

        // The install's ledger and the database merge to the higher entries.
        let five = accept(
            &fresh(),
            &target(signed_release(5, &stack(&pinned())), vec![]),
        )
        .unwrap();
        let six = accept(&five, &target(signed_release(6, &stack(&pinned())), vec![])).unwrap();
        let ledger = Arc::new(MemoryLedger::created("shop"));
        ledger.store(&six.ledger).await.unwrap();
        let (_, kept) = find_with(&cluster(settings_bytes()), ledger, Some(&five))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(kept.ledger, six.ledger);
        assert_eq!(kept.signer_lists, five.signer_lists);
    }

    #[tokio::test]
    async fn settings_the_operator_may_not_read_are_not_unprotected() {
        let error = find(&cluster(Lookup::Denied), None).await.unwrap_err();
        assert!(
            error.message.contains("can't tell whether"),
            "{}",
            error.message
        );
    }

    #[tokio::test]
    async fn protection_is_sticky() {
        let stored = accept(
            &fresh(),
            &target(signed_release(5, &stack(&pinned())), vec![]),
        )
        .unwrap();

        // Deleting the ConfigMap after acceptance stops the Operator.
        assert!(find(&cluster(Lookup::Absent), Some(&fresh()))
            .await
            .unwrap()
            .is_none());
        let error = find(&cluster(Lookup::Absent), Some(&stored))
            .await
            .unwrap_err();
        assert!(error.message.contains("is gone"), "{}", error.message);

        // So does another fingerprint, or the install moving to another cluster.
        let mut other = signing().settings;
        other.fingerprint = alien_signing::fingerprint(&format!("sha256:{}", "ab".repeat(32)));
        let error = find(
            &cluster(Lookup::Found(other.to_json().into_bytes())),
            Some(&stored),
        )
        .await
        .unwrap_err();
        assert!(error.message.contains("differ"), "{}", error.message);
        let mut moved = cluster(settings_bytes());
        moved.uid = "ffffffff-1111-2222-3333-444455556666".to_string();
        assert!(find(&moved, Some(&stored)).await.is_err());

        // Losing the database doesn't lose protection: the install's
        // ledger still holds the trust root.
        let kept = Arc::new(MemoryLedger::created("shop"));
        kept.store(&stored.ledger).await.unwrap();
        let error = find_with(&cluster(Lookup::Absent), kept, None)
            .await
            .unwrap_err();
        assert!(error.message.contains("is gone"), "{}", error.message);

        // Settings copied from another release are refused.
        let mut copied = signing().settings;
        copied.prefix = "other".to_string();
        assert!(
            find(&cluster(Lookup::Found(copied.to_json().into_bytes())), None)
                .await
                .is_err()
        );
    }
}
