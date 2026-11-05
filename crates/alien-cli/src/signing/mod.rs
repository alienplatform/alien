//! Signing statements from the CLI.
//!
//! People sign with a device key that lives in their laptop's secure
//! hardware ([`device`]). CI signs with a key from its secret store or a cloud
//! KMS ([`kms`]). Either way the CLI first verifies the project's signer list
//! from the fingerprint pinned on this machine, so the control plane can't
//! show it a list of its own making, and it signs only bytes it built or
//! printed in full.

pub mod api;
pub mod device;
pub mod kms;

use std::path::{Path, PathBuf};

use alien_error::{AlienError, Context, IntoAlienError};
use alien_release_trust::{
    Envelope, Kind, PublicKey, Role, Signature, SignedStatement, SignerChain, SoftwareKey,
};
use serde::{Deserialize, Serialize};

use crate::error::{ErrorData, Result};
use device::{Backend, DeviceKey};

/// Environment variable holding a CI signing key as PKCS#8 PEM.
pub const SIGNING_KEY_ENV: &str = "ALIEN_SIGNING_KEY";
/// Environment variable pinning the project's signing fingerprint, for CI
/// machines that never ran `alien signing init`.
pub const SIGNING_FINGERPRINT_ENV: &str = "ALIEN_SIGNING_FINGERPRINT";
/// Environment variable overriding where signing state lives.
pub const SIGNING_HOME_ENV: &str = "ALIEN_SIGNING_HOME";

/// Where this machine keeps its device key and pinned fingerprints.
#[derive(Debug, Clone)]
pub struct SigningHome {
    root: PathBuf,
}

/// The device key this machine registered.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    /// Backend holding the key.
    pub backend: Backend,
    /// Signer id the key has, or asks for, on signer lists.
    pub signer_id: String,
    /// Display label, such as the engineer's email.
    pub label: Option<String>,
}

/// What this machine pins for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTrust {
    /// `SHA256:<hex>` fingerprint of the project's signer list version 1.
    pub fingerprint: String,
}

impl SigningHome {
    /// The default home: `$ALIEN_SIGNING_HOME`, or `<config dir>/alien/signing`.
    pub fn current() -> Result<Self> {
        let root = match std::env::var_os(SIGNING_HOME_ENV) {
            Some(root) => PathBuf::from(root),
            None => dirs::config_dir()
                .ok_or_else(|| {
                    AlienError::new(ErrorData::ConfigurationError {
                        message: format!(
                            "No configuration directory on this machine; set {SIGNING_HOME_ENV}"
                        ),
                    })
                })?
                .join("alien")
                .join("signing"),
        };
        Ok(Self { root })
    }

    fn device_dir(&self) -> PathBuf {
        self.root.join("device")
    }

    fn device_record_path(&self) -> PathBuf {
        self.root.join("device.json")
    }

    fn project_path(&self, project: &str) -> PathBuf {
        self.root.join("projects").join(format!("{project}.json"))
    }

    /// The device key, if this machine created one.
    pub fn device(&self) -> Result<Option<(DeviceRecord, DeviceKey)>> {
        let Some(record) = read_json::<DeviceRecord>(&self.device_record_path())? else {
            return Ok(None);
        };
        let key = DeviceKey::open(record.backend, &self.device_dir())?;
        Ok(Some((record, key)))
    }

    /// Create the device key on `backend` (or the first available one).
    pub fn create_device(
        &self,
        backend: Option<Backend>,
        signer_id: &str,
        label: Option<String>,
    ) -> Result<(DeviceRecord, DeviceKey)> {
        if self.device()?.is_some() {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "This machine already has a device key in {}",
                    self.root.display()
                ),
            }));
        }
        check_signer_id(signer_id)?;
        let backend = match backend {
            Some(backend) => backend,
            None => Backend::ALL
                .into_iter()
                .find(|backend| backend.available())
                .unwrap_or(Backend::Software),
        };
        let key = DeviceKey::create(backend, &self.device_dir())?;
        let record = DeviceRecord {
            backend,
            signer_id: signer_id.to_string(),
            label,
        };
        write_json(&self.device_record_path(), &record)?;
        Ok((record, key))
    }

    /// The fingerprint pinned for `project` on this machine.
    pub fn fingerprint(&self, project: &str) -> Result<Option<String>> {
        Ok(read_json::<ProjectTrust>(&self.project_path(project))?.map(|trust| trust.fingerprint))
    }

    /// Pin `fingerprint` for `project` on this machine. A different pin is
    /// never replaced silently.
    pub fn pin(&self, project: &str, fingerprint: &str) -> Result<()> {
        if let Some(existing) = read_json::<ProjectTrust>(&self.project_path(project))? {
            if existing.fingerprint != fingerprint {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "Project '{project}' is pinned to {} on this machine, not {fingerprint}; remove {} to change it",
                        existing.fingerprint,
                        self.project_path(project).display()
                    ),
                }));
            }
            return Ok(());
        }
        write_json(
            &self.project_path(project),
            &ProjectTrust {
                fingerprint: fingerprint.to_string(),
            },
        )
    }
}

/// A key that signs statements.
pub enum StatementSigner {
    /// A key in memory: a CI key from its secret, or the recovery key.
    Software { id: String, key: SoftwareKey },
    /// This machine's device key.
    Device { id: String, key: DeviceKey },
    /// A key in AWS KMS.
    Kms { id: String, key: kms::KmsKey },
}

impl StatementSigner {
    /// Signer id the signatures name.
    pub fn id(&self) -> &str {
        match self {
            Self::Software { id, .. } | Self::Device { id, .. } | Self::Kms { id, .. } => id,
        }
    }

    /// The public key.
    pub fn public_key(&self) -> PublicKey {
        match self {
            Self::Software { key, .. } => key.public_key(),
            Self::Device { key, .. } => key.public_key.clone(),
            Self::Kms { key, .. } => key.public_key().clone(),
        }
    }

    /// Sign `payload` bytes.
    pub async fn sign(&self, payload: &[u8]) -> Result<Signature> {
        let sig = match self {
            Self::Software { key, .. } => key.sign(payload),
            Self::Device { key, .. } => key.sign(payload)?,
            Self::Kms { key, .. } => key.sign(payload).await?,
        };
        Ok(Signature {
            signer: self.id().to_string(),
            sig,
        })
    }
}

/// Where a command takes its signing key from.
#[derive(Debug, Clone, Default, clap::Args)]
pub struct SignerArgs {
    /// Sign with this PKCS#8 PEM key file instead of the device key, such as a
    /// CI key. `ALIEN_SIGNING_KEY` may hold the PEM instead.
    #[arg(long, env = "ALIEN_SIGNING_KEY_FILE")]
    pub signing_key_file: Option<PathBuf>,

    /// Sign with this AWS KMS key (ARN, id or alias) instead of the device key.
    #[arg(long, conflicts_with = "signing_key_file")]
    pub kms_key: Option<String>,

    /// Signer id the key has on the signer list. Defaults to `ci` for key files
    /// and KMS keys, and to the device's own id.
    #[arg(long)]
    pub signer_id: Option<String>,

    /// The project's signing fingerprint, for machines that never pinned it,
    /// such as CI. The signer list is verified from it before signing.
    #[arg(long = "signing-fingerprint", env = SIGNING_FINGERPRINT_ENV)]
    pub fingerprint: Option<String>,
}

impl SignerArgs {
    /// Open the signer these arguments name.
    pub async fn open(&self, home: &SigningHome) -> Result<StatementSigner> {
        if let Some(path) = &self.signing_key_file {
            let pem = std::fs::read_to_string(path).into_alien_error().context(
                ErrorData::FileOperationFailed {
                    operation: "read".to_string(),
                    file_path: path.display().to_string(),
                    reason: "Failed to read the signing key file".to_string(),
                },
            )?;
            return software(&pem, self.signer_id.clone());
        }
        if let Ok(pem) = std::env::var(SIGNING_KEY_ENV) {
            if self.kms_key.is_none() {
                return software(&pem, self.signer_id.clone());
            }
        }
        if let Some(key_id) = &self.kms_key {
            let key = kms::KmsKey::open(key_id).await?;
            return Ok(StatementSigner::Kms {
                id: self.signer_id.clone().unwrap_or_else(|| "ci".to_string()),
                key,
            });
        }
        let (record, key) = home.device()?.ok_or_else(|| {
            AlienError::new(ErrorData::ConfigurationError {
                message: "This machine has no device key; run `alien signing device` (or `alien login`), or pass --signing-key-file".to_string(),
            })
        })?;
        Ok(StatementSigner::Device {
            id: self.signer_id.clone().unwrap_or(record.signer_id),
            key,
        })
    }
}

fn software(pem: &str, id: Option<String>) -> Result<StatementSigner> {
    let key = SoftwareKey::from_pkcs8_pem(pem).context(ErrorData::SigningFailed {
        message: "The signing key is not a PKCS#8 PEM P-256 key".to_string(),
    })?;
    Ok(StatementSigner::Software {
        id: id.unwrap_or_else(|| "ci".to_string()),
        key,
    })
}

/// The project's signer chain, verified from the fingerprint pinned here.
pub async fn verified_chain(
    api: &api::SigningApi,
    home: &SigningHome,
    project: &str,
    pinned: Option<&str>,
) -> Result<SignerChain> {
    let fingerprint = match pinned {
        Some(fingerprint) => Some(fingerprint.trim().to_string()),
        None => home.fingerprint(project)?,
    };
    let fingerprint = fingerprint.ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: format!(
                "No signing fingerprint is pinned for project '{project}' on this machine; run `alien signers trust <fingerprint>` with the fingerprint from `alien signing init`, or set {SIGNING_FINGERPRINT_ENV}"
            ),
        })
    })?;
    let statements = api.signer_lists(project).await?;
    SignerChain::from_statements(project, &fingerprint, &statements).context(ErrorData::SigningFailed {
        message: format!(
            "The signer lists the server returned for '{project}' do not verify from the pinned fingerprint {fingerprint}"
        ),
    })
}

/// Refuse to sign as a signer the current list doesn't give `role`, so a
/// statement enforcers would refuse is caught before it is sent.
pub fn check_signer(chain: &SignerChain, signer: &StatementSigner, role: Role) -> Result<()> {
    let list = chain.current();
    let (key, roles) = list.signer(signer.id()).ok_or_else(|| {
        AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "'{}' is not on signer list version {}; ask an admin to run `alien signers approve {}`",
                signer.id(),
                list.version,
                signer.id()
            ),
        })
    })?;
    if *key != signer.public_key() {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "Signer '{}' on the list has another key than the one here",
                signer.id()
            ),
        }));
    }
    if !roles.contains(&role) {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "Signer '{}' does not hold the '{}' role",
                signer.id(),
                role.as_str()
            ),
        }));
    }
    Ok(())
}

/// Sign `envelope` with `signer`.
pub async fn sign_envelope<B: Serialize>(
    envelope: &Envelope<B>,
    signer: &StatementSigner,
) -> Result<SignedStatement> {
    let payload = envelope.to_payload().context(ErrorData::SigningFailed {
        message: "The statement could not be encoded".to_string(),
    })?;
    let mut statement = SignedStatement::new(&payload);
    statement.signatures.push(signer.sign(&payload).await?);
    Ok(statement)
}

/// A sequence for a new statement: the time in milliseconds. Sequences only
/// need to grow per kind, and CI and laptops share a clock source closely
/// enough that the later release wins.
pub fn new_sequence() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

/// The envelope header for a statement of `kind`.
pub fn envelope<B>(
    kind: Kind,
    project: &str,
    deployment: Option<&str>,
    chain: &SignerChain,
    body: B,
) -> Envelope<B> {
    Envelope {
        kind: kind.as_str().to_string(),
        project: Some(project.to_string()),
        deployment: deployment.map(str::to_string),
        sequence: new_sequence(),
        not_after: None,
        signer_list_version: chain.current().version,
        body,
    }
}

fn check_signer_id(id: &str) -> Result<()> {
    let valid = (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-'));
    if !valid {
        return Err(AlienError::new(ErrorData::ValidationError {
            field: "signer-id".to_string(),
            message: format!("'{id}' must be 1-64 of [a-z0-9-]"),
        }));
    }
    Ok(())
}

/// A default signer id for this machine: `<user>-<host>`, lower case.
pub fn default_device_id() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string());
    let host = hostname();
    let id: String = format!("{user}-{host}")
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    id.trim_matches('-').chars().take(64).collect()
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .map(|name| {
            name.trim()
                .split('.')
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "device".to_string())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "parse".to_string(),
                file_path: path.display().to_string(),
                reason: "The signing state file is not valid JSON".to_string(),
            })
            .map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .into_alien_error()
            .context(ErrorData::FileOperationFailed {
                operation: "read".to_string(),
                file_path: path.display().to_string(),
                reason: "Failed to read signing state".to_string(),
            }),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).into_alien_error().context(
            ErrorData::FileOperationFailed {
                operation: "create".to_string(),
                file_path: parent.display().to_string(),
                reason: "Failed to create the signing directory".to_string(),
            },
        )?;
    }
    let text = serde_json::to_string_pretty(value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "signing state".to_string(),
        })?;
    std::fs::write(path, text)
        .into_alien_error()
        .context(ErrorData::FileOperationFailed {
            operation: "write".to_string(),
            file_path: path.display().to_string(),
            reason: "Failed to write signing state".to_string(),
        })
}
