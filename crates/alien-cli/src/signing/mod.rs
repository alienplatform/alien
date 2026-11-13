//! Signing statements from the CLI.
//!
//! People sign with a device key that lives in their laptop's secure
//! hardware ([`device`]). CI signs with the key the `release` environment of
//! its repository holds. Either way the CLI first verifies the project's
//! signer list from the fingerprint the repository pins in
//! `.alien/signing.json`, so the control plane can't show it a list of its
//! own making, and it writes every statement header itself.

pub mod api;
pub mod device;
pub mod release;

use std::path::{Path, PathBuf};

use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::{
    Envelope, Kind, PublicKey, Role, SecretKey, Signature, SignedStatement, Signer, SignerChain,
    SignerKind, SignerListBody, TrustedList,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{ErrorData, Result};
use device::{Backend, DeviceKey};

/// Environment variable holding the CI signing key as PKCS#8 PEM. The
/// `release` environment of the vendor's repository stores it.
pub const SIGNING_KEY_ENV: &str = "ALIEN_SIGNING_KEY";
/// Environment variable overriding where this machine keeps its device key.
pub const SIGNING_HOME_ENV: &str = "ALIEN_SIGNING_HOME";
/// The file, relative to the repository, that pins the project's signing
/// fingerprint. The CLI writes it on the first release and never rewrites it.
pub const REPO_SIGNING_FILE: &str = ".alien/signing.json";
/// How long a signer list version stays valid.
pub const LIST_LIFETIME_DAYS: i64 = 90;
/// Under this many days left, CI warns that an admin must refresh the list.
pub const LIST_WARNING_DAYS: i64 = 30;

/// What a repository pins: the fingerprint of its project's signer list
/// version 1. Committed with the code, owned by the CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepoSigning {
    /// `SHA256:<hex>` of the signer list version 1 payload.
    pub fingerprint: String,
    /// The project the list belongs to.
    pub project: String,
}

impl RepoSigning {
    fn path(dir: &Path) -> PathBuf {
        dir.join(REPO_SIGNING_FILE)
    }

    /// The pin in `dir`, if the repository has one.
    pub fn read(dir: &Path) -> Result<Option<Self>> {
        let path = Self::path(dir);
        let Some(repo) = read_json::<Self>(&path)? else {
            return Ok(None);
        };
        if !alien_signing::signer_list::is_fingerprint(&repo.fingerprint) {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "{} pins '{}', which is not a signing fingerprint",
                    path.display(),
                    repo.fingerprint
                ),
            }));
        }
        Ok(Some(repo))
    }

    /// Write the pin into `dir`. An existing different pin is never replaced.
    pub fn write(&self, dir: &Path) -> Result<()> {
        if let Some(existing) = Self::read(dir)? {
            if &existing != self {
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "{} already pins {} for project '{}'",
                        Self::path(dir).display(),
                        existing.fingerprint,
                        existing.project
                    ),
                }));
            }
            return Ok(());
        }
        write_json(&Self::path(dir), self)?;
        allow_in_gitignore(dir)
    }
}

/// `.alien` holds build output and is ignored; keep ignoring everything in it
/// but the signing pin, which is committed. Git can't re-include a file whose
/// directory is ignored, so `.alien` becomes `.alien/*`.
fn allow_in_gitignore(dir: &Path) -> Result<()> {
    let path = dir.join(".gitignore");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .into_alien_error()
                .context(file_failed(&path, "read .gitignore"))
        }
    };
    let keep = format!("!{REPO_SIGNING_FILE}");
    if content.lines().any(|line| line.trim() == keep) {
        return Ok(());
    }
    let mut lines: Vec<String> = Vec::new();
    let mut ignored = false;
    for line in content.lines() {
        if matches!(line.trim(), ".alien" | ".alien/" | "/.alien" | "/.alien/") {
            lines.push(".alien/*".to_string());
            ignored = true;
        } else {
            ignored |= line.trim() == ".alien/*";
            lines.push(line.to_string());
        }
    }
    if !ignored {
        return Ok(());
    }
    lines.push(keep);
    std::fs::write(&path, format!("{}\n", lines.join("\n")))
        .into_alien_error()
        .context(file_failed(&path, "update .gitignore"))
}

/// Where this machine keeps its device key.
#[derive(Debug, Clone)]
pub struct SigningHome {
    root: PathBuf,
}

/// The device key this machine created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    /// Backend holding the key.
    pub backend: Backend,
    /// Signer id the device asks for on signer lists.
    pub signer_id: String,
    /// Label admins see, such as the engineer's email.
    pub label: Option<String>,
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

    /// The device key, if this machine created one.
    pub fn device(&self) -> Result<Option<(DeviceRecord, DeviceKey)>> {
        let Some(record) = read_json::<DeviceRecord>(&self.device_record_path())? else {
            return Ok(None);
        };
        let key = DeviceKey::open(record.backend, &self.device_dir())?;
        Ok(Some((record, key)))
    }

    /// This machine's device key, created on the first hardware backend it
    /// has. `None` when it has no secure hardware.
    pub fn device_or_create(&self) -> Result<Option<(DeviceRecord, DeviceKey)>> {
        if let Some(device) = self.device()? {
            return Ok(Some(device));
        }
        let Some(backend) = Backend::detect() else {
            return Ok(None);
        };
        let key = DeviceKey::create(backend, &self.device_dir())?;
        let record = DeviceRecord {
            backend,
            signer_id: default_device_id(),
            label: device_label(),
        };
        write_json(&self.device_record_path(), &record)?;
        Ok(Some((record, key)))
    }
}

/// A key that signs statements.
pub enum StatementSigner {
    /// This machine's device key.
    Device {
        /// Signer id on the list.
        id: String,
        /// The key.
        key: DeviceKey,
    },
    /// A key in memory: the CI key, or the key the recovery code derives.
    Key {
        /// Signer id on the list.
        id: String,
        /// The key.
        key: SecretKey,
    },
}

impl StatementSigner {
    /// Signer id the signatures name.
    pub fn id(&self) -> &str {
        match self {
            Self::Device { id, .. } | Self::Key { id, .. } => id,
        }
    }

    /// The public key.
    pub fn public_key(&self) -> PublicKey {
        match self {
            Self::Device { key, .. } => key.public_key.clone(),
            Self::Key { key, .. } => key.public_key(),
        }
    }

    /// What the person does to sign, for prompts; `None` for keys in memory.
    pub fn gesture(&self) -> Option<&'static str> {
        match self {
            Self::Device { key, .. } => Some(key.backend.gesture()),
            Self::Key { .. } => None,
        }
    }

    /// Sign `payload` bytes.
    pub fn sign(&self, payload: &[u8]) -> Result<Signature> {
        let sig = match self {
            Self::Device { key, .. } => key.sign(payload)?,
            Self::Key { key, .. } => key.sign(payload),
        };
        Ok(Signature {
            signer: self.id().to_string(),
            sig,
        })
    }
}

/// The CI key from [`SIGNING_KEY_ENV`], if set. Its signer id is whichever
/// signer on the list holds its public key.
pub fn ci_key() -> Result<Option<SecretKey>> {
    match std::env::var(SIGNING_KEY_ENV) {
        Ok(pem) => SecretKey::from_pkcs8_pem(&pem)
            .context(ErrorData::SigningFailed {
                message: format!("{SIGNING_KEY_ENV} is not a PKCS#8 PEM P-256 key"),
            })
            .map(Some),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(AlienError::new(ErrorData::SigningFailed {
            message: format!("{SIGNING_KEY_ENV} is not valid UTF-8"),
        })),
    }
}

/// The signer on `list` holding `key`.
pub fn signer_with_key<'a>(list: &'a TrustedList, key: &PublicKey) -> Option<&'a Signer> {
    list.body
        .signers
        .iter()
        .find(|signer| PublicKey::parse(&signer.key).is_ok_and(|parsed| &parsed == key))
}

/// The project's signer chain, verified from the repository's pin. A server
/// that shows another chain, or one for another project, is refused.
pub async fn verified_chain(api: &api::SigningApi, repo: &RepoSigning) -> Result<SignerChain> {
    let statements = api.signer_lists(&repo.project).await?;
    let chain = SignerChain::from_statements(&repo.fingerprint, &statements, Utc::now()).context(
        ErrorData::SigningFailed {
            message: format!(
                "The device lists the server returned for '{}' do not verify from {} in {REPO_SIGNING_FILE}",
                repo.project, repo.fingerprint
            ),
        },
    )?;
    if chain.project() != repo.project {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "{REPO_SIGNING_FILE} names project '{}', but its device list is for '{}'",
                repo.project,
                chain.project()
            ),
        }));
    }
    Ok(chain)
}

/// The key that signs config (`Role::Release`) or sessions (`Role::Operate`)
/// here, verified on the project's list from `.alien/signing.json`: the CI
/// key when one is set, otherwise this machine's device key.
pub async fn project_signer(
    api: &api::SigningApi,
    role: Role,
) -> Result<(SignerChain, StatementSigner)> {
    let dir = crate::get_current_dir()?;
    let repo = RepoSigning::read(&dir)?.ok_or_else(|| {
        AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "This deployment is protected, so this must be signed; run it from the repo with {REPO_SIGNING_FILE}"
            ),
        })
    })?;
    let chain = verified_chain(api, &repo).await?;
    let list = chain.current();
    let not_trusted = || {
        AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "This device isn't trusted for {}; ask an admin, then run this again",
                repo.project
            ),
        })
    };
    let signer = match ci_key()? {
        Some(key) => {
            let id = signer_with_key(list, &key.public_key())
                .ok_or_else(not_trusted)?
                .id
                .clone();
            StatementSigner::Key { id, key }
        }
        None => {
            let (_, key) = SigningHome::current()?.device()?.ok_or_else(not_trusted)?;
            let id = signer_with_key(list, &key.public_key)
                .ok_or_else(not_trusted)?
                .id
                .clone();
            StatementSigner::Device { id, key }
        }
    };
    if !holds(list, &signer, role) {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "{} doesn't hold the {} role for {}",
                signer.id(),
                role.as_str(),
                repo.project
            ),
        }));
    }
    Ok((chain, signer))
}

/// The install id of a deployment, from the control plane's view of it.
/// It is only a claim: an install that derives another id from its own
/// identity refuses what is signed for this one.
pub fn install_id(deployment: &serde_json::Value) -> Result<alien_signing::InstallId> {
    let text = |value: &serde_json::Value| value.as_str().map(str::to_string);
    let prefix = text(&deployment["stackState"]["resourcePrefix"]);
    let environment = &deployment["environmentInfo"];
    let id = match text(&deployment["installId"]) {
        Some(id) => Some(id),
        None => match (environment["platform"].as_str(), prefix) {
            (Some("aws"), Some(prefix)) => text(&environment["accountId"])
                .zip(text(&environment["region"]))
                .map(|(account, region)| format!("aws:{account}:{region}:{prefix}")),
            (Some("gcp"), Some(prefix)) => {
                text(&environment["projectId"]).map(|project| format!("gcp:{project}:{prefix}"))
            }
            _ => None,
        },
    };
    let id = id.ok_or_else(|| {
        AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "Can't tell which install deployment {} is: it hasn't reported its cloud account and prefix yet",
                deployment["id"].as_str().unwrap_or("?")
            ),
        })
    })?;
    alien_signing::InstallId::parse(&id).context(ErrorData::SigningFailed {
        message: format!("The deployment reports install '{id}', which is not an install id"),
    })
}

/// Whether `signer` may sign `role` statements on the current list.
pub fn holds(list: &TrustedList, signer: &StatementSigner, role: Role) -> bool {
    list.signer(signer.id())
        .is_some_and(|(key, roles)| *key == signer.public_key() && roles.contains(&role))
}

/// Sign `envelope` with `signer`.
pub fn sign_envelope<B: Serialize>(
    envelope: &Envelope<B>,
    signer: &StatementSigner,
) -> Result<SignedStatement> {
    let payload = envelope.to_payload().context(ErrorData::SigningFailed {
        message: "The statement could not be encoded".to_string(),
    })?;
    let mut statement = SignedStatement::new(&payload);
    statement.signatures.push(signer.sign(&payload)?);
    Ok(statement)
}

/// A sequence for a new statement: the time in milliseconds. Sequences only
/// need to grow per kind, and CI and laptops share a clock closely enough
/// that the later statement wins.
pub fn new_sequence() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0)
}

/// The header for a statement of `kind` signed against `chain`'s current
/// list. `install` is set exactly for the kinds that name one.
pub fn envelope<B>(
    kind: Kind,
    chain: &SignerChain,
    install: Option<&alien_signing::InstallId>,
    body: B,
) -> Envelope<B> {
    Envelope {
        kind: kind.as_str().to_string(),
        project: chain.project().to_string(),
        install: install.map(ToString::to_string),
        sequence: new_sequence(),
        not_after: None,
        signer_list_version: chain.current().version,
        body,
    }
}

/// The next signer list version after `chain`'s current one, with `body`
/// linked to it and a fresh expiry.
pub fn next_list(chain: &SignerChain, mut body: SignerListBody) -> Envelope<SignerListBody> {
    let current = chain.current();
    body.previous = Some(current.digest.clone());
    Envelope {
        kind: Kind::SignerList.as_str().to_string(),
        project: chain.project().to_string(),
        install: None,
        sequence: current.version + 1,
        not_after: Some(list_expiry(Utc::now())),
        signer_list_version: current.version,
        body,
    }
}

/// When a list signed at `now` expires.
pub fn list_expiry(now: DateTime<Utc>) -> DateTime<Utc> {
    now + Duration::days(LIST_LIFETIME_DAYS)
}

/// The 16-character code a teammate sends an admin, which the admin types
/// to approve the request. It covers the key, roles, kind and label the
/// request adds and the pending change's id, so a control plane that alters
/// any of them, or replays the code on another request, is caught. 80 bits,
/// Crockford base32, in groups of four.
pub fn pairing_code(signer: &Signer, change_id: &str) -> String {
    let roles: Vec<&str> = signer.roles.iter().map(|role| role.as_str()).collect();
    let mut hasher = Sha256::new();
    for part in [
        "alien pairing code v1",
        signer.key.as_str(),
        &roles.join(","),
        kind_name(signer.kind),
        signer.label.as_deref().unwrap_or(""),
        signer.id.as_str(),
        change_id,
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let text = alien_signing::recovery::encode(&digest[..10]);
    text.as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// Whether `typed` is `expected`, ignoring case, spaces and dashes, and
/// reading the letters Crockford base32 confuses with digits as digits.
pub fn same_code(typed: &str, expected: &str) -> bool {
    fn normalize(text: &str) -> String {
        text.chars()
            .filter(|character| !matches!(character, '-' | ' ' | '\t'))
            .map(|character| match character.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                other => other,
            })
            .collect()
    }
    normalize(typed) == normalize(expected)
}

/// The kind's name as lists show it.
pub fn kind_name(kind: SignerKind) -> &'static str {
    match kind {
        SignerKind::Device => "device",
        SignerKind::Ci => "ci",
        SignerKind::Recovery => "recovery",
    }
}

/// A default signer id for this machine: `<user>-<host>`, lower case.
pub fn default_device_id() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string());
    let id: String = format!("{user}-{}", hostname())
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    id.trim_matches('-').chars().take(64).collect()
}

/// The label admins see next to this device: the git email, if set.
fn device_label() -> Option<String> {
    std::process::Command::new("git")
        .args(["config", "user.email"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|email| !email.is_empty())
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
        })
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
                reason: "The signing file is not valid JSON".to_string(),
            })
            .map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .into_alien_error()
            .context(file_failed(path, "read signing state")),
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .into_alien_error()
            .context(file_failed(parent, "create the directory"))?;
    }
    let text = serde_json::to_string_pretty(value)
        .into_alien_error()
        .context(ErrorData::JsonError {
            operation: "serialize".to_string(),
            reason: "signing state".to_string(),
        })?;
    std::fs::write(path, format!("{text}\n"))
        .into_alien_error()
        .context(file_failed(path, "write signing state"))
}

fn file_failed(path: &Path, reason: &str) -> ErrorData {
    ErrorData::FileOperationFailed {
        operation: reason.to_string(),
        file_path: path.display().to_string(),
        reason: reason.to_string(),
    }
}

/// Check a statement the server returned: of `kind` for this project (and
/// `install`, when given), not deny-listed, signed with the kind's role by
/// the signer list version it names. The reason when it isn't.
pub fn check_statement(
    chain: &SignerChain,
    opened: &alien_signing::Opened,
    kind: Kind,
    install: Option<&alien_signing::InstallId>,
    now: DateTime<Utc>,
) -> std::result::Result<(), String> {
    if opened.kind != kind || opened.project != chain.project() {
        return Err(format!(
            "it is a {} statement of project {}",
            opened.kind.as_str(),
            opened.project
        ));
    }
    if let Some(install) = install {
        if opened.install.as_deref() != Some(install.to_string().as_str()) {
            return Err(format!(
                "it is for install {}, not {install}",
                opened.install.as_deref().unwrap_or("none")
            ));
        }
    }
    if chain.current().denies(&opened.digest) {
        return Err("it is deny-listed".to_string());
    }
    let version = opened.signer_list_version;
    let first = chain
        .digest_of(1)
        .ok_or_else(|| "the signer list chain is empty".to_string())?;
    if version == 0 || version > chain.current().version {
        return Err(format!("it names signer list v{version}"));
    }
    let named = SignerChain::from_statements(
        &alien_signing::fingerprint(first),
        &chain.statements()[..version as usize],
        now,
    )
    .map_err(|error| format!("signer list v{version} doesn't verify ({error})"))?;
    named
        .current()
        .check_signatures(opened, kind.role())
        .map_err(|error| {
            format!(
                "it isn't signed by a {} key ({error})",
                kind.role().as_str()
            )
        })?;
    Ok(())
}

/// The body of the config statement to build the next one on, checked here:
/// a config statement of this project for this install, not deny-listed,
/// signed with the release role by the list version it names. It is the
/// newest statement the server holds; `None` when there is none. A statement
/// that doesn't check out is refused, so a change never drops what the last
/// signed config set.
pub fn latest_config(
    chain: &SignerChain,
    install: &alien_signing::InstallId,
    statements: &[SignedStatement],
    now: DateTime<Utc>,
) -> Result<Option<alien_signing::ConfigBody>> {
    let refused = |reason: String| {
        AlienError::new(ErrorData::SigningFailed {
            message: format!(
                "The deployment's latest signed config doesn't check out: {reason}. Nothing was signed."
            ),
        })
    };
    let mut opened = statements
        .iter()
        .map(SignedStatement::open)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| refused(format!("a stored statement doesn't parse ({error})")))?;
    opened.sort_by_key(|opened| opened.sequence);
    let newest = opened.pop();
    let Some(newest) = newest else {
        return Ok(None);
    };
    check_statement(chain, &newest, Kind::Config, Some(install), now).map_err(refused)?;
    let body: alien_signing::ConfigBody = newest
        .body()
        .map_err(|error| refused(format!("its body doesn't parse ({error})")))?;
    Ok(Some(body))
}

/// Refuse `config` unless the release it will be applied with accepts it.
/// `statement` is the release `release_id` signed for the deployment's
/// platform, as the server returned it; it is checked here first. The
/// install runs the same check and refuses every update while the newest
/// config names something the release doesn't make configurable, so a
/// config it would refuse is never signed.
pub fn check_config_for_release(
    chain: &SignerChain,
    release_id: &str,
    statement: &SignedStatement,
    config: &alien_signing::ConfigBody,
    now: DateTime<Utc>,
) -> Result<()> {
    let release: alien_signing::ReleaseBody = statement
        .open()
        .map_err(|error| format!("it doesn't open ({error})"))
        .and_then(|opened| {
            check_statement(chain, &opened, Kind::Release, None, now)?;
            opened
                .body()
                .map_err(|error| format!("its body doesn't parse ({error})"))
        })
        .map_err(|reason| {
            AlienError::new(ErrorData::SigningFailed {
                message: format!(
                    "The deployment's release {release_id} doesn't check out: {reason}. Nothing was signed."
                ),
            })
        })?;
    config.check(&release).map_err(|error| {
        let listed = |names: &[String]| {
            if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            }
        };
        AlienError::new(ErrorData::ValidationError {
            field: "config".to_string(),
            message: format!(
                "{}. Release {release_id} lets you set env: {}; inputs: {}. Nothing was signed.",
                error.message,
                listed(&release.configurable.env),
                listed(&release.configurable.inputs),
            ),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_v1(now: DateTime<Utc>) -> SignedStatement {
        let alice = SecretKey::from_secret_bytes(&[1; 32]).unwrap();
        SignedStatement::new(
            &Envelope {
                kind: Kind::SignerList.as_str().to_string(),
                project: "prj".to_string(),
                install: None,
                sequence: 1,
                not_after: Some(now + Duration::days(30)),
                signer_list_version: 1,
                body: SignerListBody {
                    previous: None,
                    admin_threshold: 1,
                    signers: vec![Signer {
                        id: "alice".to_string(),
                        key: alice.public_key().as_str().to_string(),
                        kind: SignerKind::Device,
                        roles: vec![Role::Admin, Role::Release],
                        label: None,
                    }],
                    deny: Vec::new(),
                    recovered: false,
                },
            }
            .to_payload()
            .unwrap(),
        )
        .sign_with("alice", &alice)
        .unwrap()
    }

    fn config(
        chain: &SignerChain,
        install: &alien_signing::InstallId,
        sequence: u64,
        body: alien_signing::ConfigBody,
        seed: u8,
    ) -> SignedStatement {
        let mut envelope = envelope(Kind::Config, chain, Some(install), body);
        envelope.sequence = sequence;
        SignedStatement::new(&envelope.to_payload().unwrap())
            .sign_with("alice", &SecretKey::from_secret_bytes(&[seed; 32]).unwrap())
            .unwrap()
    }

    #[test]
    fn a_config_change_keeps_what_the_last_signed_config_set() {
        let now = Utc::now();
        let v1 = list_v1(now);
        let fingerprint = alien_signing::fingerprint(&v1.open().unwrap().digest);
        let chain = SignerChain::from_statements(&fingerprint, &[v1], now).unwrap();
        let install = alien_signing::InstallId::parse("aws:111122223333:us-east-1:acme").unwrap();
        assert_eq!(latest_config(&chain, &install, &[], now).unwrap(), None);

        // set-env, then set-inputs built on it: both halves are kept.
        let env = alien_signing::ConfigBody {
            env: [("LOG_LEVEL".to_string(), "debug".to_string())].into(),
            ..Default::default()
        };
        let first = config(&chain, &install, 10, env.clone(), 1);
        let mut next = latest_config(&chain, &install, &[first.clone()], now)
            .unwrap()
            .unwrap();
        assert_eq!(next, env);
        next.inputs
            .insert("region".to_string(), serde_json::json!("eu"));
        let second = config(&chain, &install, 11, next, 1);
        let latest = latest_config(&chain, &install, &[second.clone(), first.clone()], now)
            .unwrap()
            .unwrap();
        assert_eq!(latest.env["LOG_LEVEL"], "debug");
        assert_eq!(latest.inputs["region"], "eu");

        // A newer statement the server made up, or one for another install,
        // is refused rather than built on.
        let forged = config(&chain, &install, 12, Default::default(), 9);
        let error = latest_config(&chain, &install, &[first.clone(), forged], now).unwrap_err();
        assert!(
            error.message.contains("isn't signed by a release key"),
            "{}",
            error.message
        );
        let other = alien_signing::InstallId::parse("aws:111122223333:us-east-1:other").unwrap();
        let elsewhere = config(&chain, &other, 13, Default::default(), 1);
        let error = latest_config(&chain, &install, &[first, elsewhere], now).unwrap_err();
        assert!(
            error
                .message
                .contains("not aws:111122223333:us-east-1:acme"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_config_the_release_would_refuse_is_never_signed() {
        let now = Utc::now();
        let v1 = list_v1(now);
        let fingerprint = alien_signing::fingerprint(&v1.open().unwrap().digest);
        let chain = SignerChain::from_statements(&fingerprint, &[v1], now).unwrap();
        let release_body = alien_signing::ReleaseBody {
            platform: "gcp".to_string(),
            channel: "production".to_string(),
            workloads: Vec::new(),
            configurable: alien_signing::release::Configurable {
                env: vec!["LOG_LEVEL".to_string()],
                inputs: Vec::new(),
            },
            components: Default::default(),
            setup: Default::default(),
            stack: None,
        };
        let release = |seed: u8| {
            SignedStatement::new(
                &envelope(Kind::Release, &chain, None, release_body.clone())
                    .to_payload()
                    .unwrap(),
            )
            .sign_with("alice", &SecretKey::from_secret_bytes(&[seed; 32]).unwrap())
            .unwrap()
        };
        let config = |name: &str| alien_signing::ConfigBody {
            env: [(name.to_string(), "1".to_string())].into(),
            ..Default::default()
        };

        check_config_for_release(&chain, "rel_1", &release(1), &config("LOG_LEVEL"), now)
            .expect("a configurable name is signed");

        // A name the release doesn't declare: the install would refuse every
        // update until a new config replaced it.
        let error =
            check_config_for_release(&chain, "rel_1", &release(1), &config("V8_PROBE"), now)
                .unwrap_err();
        assert_eq!(error.code, "VALIDATION_ERROR");
        assert_eq!(
            error.message,
            "Validation failed for config: The 'config/v1' statement was refused: env \
             V8_PROBE is not configurable in the signed release. Release rel_1 lets you set \
             env: LOG_LEVEL; inputs: none. Nothing was signed."
        );

        // The release's word is checked before it is trusted.
        let forged =
            check_config_for_release(&chain, "rel_1", &release(9), &config("LOG_LEVEL"), now)
                .unwrap_err();
        assert!(
            forged
                .message
                .contains("release rel_1 doesn't check out: it isn't signed by a release key"),
            "{}",
            forged.message
        );
    }

    fn signer(roles: Vec<Role>, label: Option<&str>) -> Signer {
        Signer {
            id: "bob-thinkpad".to_string(),
            key: SecretKey::from_secret_bytes(&[9; 32])
                .unwrap()
                .public_key()
                .as_str()
                .to_string(),
            kind: SignerKind::Device,
            roles,
            label: label.map(str::to_string),
        }
    }

    #[test]
    fn a_pairing_code_changes_with_every_field_it_covers() {
        let base = signer(vec![Role::Release], Some("bob@example.com"));
        let code = pairing_code(&base, "pch_1");
        assert_eq!(code.len(), 19, "16 characters in four groups: {code}");
        assert!(code
            .split('-')
            .all(|group| group.len() == 4 && group.chars().all(|c| c.is_ascii_alphanumeric())));

        let mut admin = base.clone();
        admin.roles = vec![Role::Release, Role::Admin];
        let mut relabeled = base.clone();
        relabeled.label = Some("mallory@example.com".to_string());
        let mut ci = base.clone();
        ci.kind = SignerKind::Ci;
        let mut other_key = base.clone();
        other_key.key = SecretKey::from_secret_bytes(&[10; 32])
            .unwrap()
            .public_key()
            .as_str()
            .to_string();
        for changed in [&admin, &relabeled, &ci, &other_key] {
            assert_ne!(pairing_code(changed, "pch_1"), code, "{changed:?}");
        }
        assert_ne!(pairing_code(&base, "pch_2"), code, "another request");
        assert_eq!(pairing_code(&base, "pch_1"), code, "deterministic");
    }

    #[test]
    fn a_typed_code_is_compared_without_dashes_or_case() {
        assert!(same_code("4f2a 9kq7-xm3d-t8vb", "4F2A-9KQ7-XM3D-T8VB"));
        assert!(same_code("4F2A9KQ7XM3DT8VB", "4F2A-9KQ7-XM3D-T8VB"));
        assert!(same_code("O1L1-0000-0000-0000", "0111-0000-0000-0000"));
        assert!(!same_code("4F2A-9KQ7-XM3D-T8VC", "4F2A-9KQ7-XM3D-T8VB"));
        assert!(!same_code("4F2A-9KQ7-XM3D", "4F2A-9KQ7-XM3D-T8VB"));
    }

    #[test]
    fn the_pin_is_kept_out_of_the_ignored_build_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".gitignore"), "node_modules/\n.alien\n").unwrap();
        let repo = RepoSigning {
            fingerprint: format!("SHA256:{}", "a".repeat(64)),
            project: "my-app".to_string(),
        };
        repo.write(dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
            "node_modules/\n.alien/*\n!.alien/signing.json\n"
        );
        assert_eq!(RepoSigning::read(dir.path()).unwrap(), Some(repo.clone()));
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::write(dir.path().join(".alien/build.json"), "{}").unwrap();
        let ignored = |path: &str| {
            std::process::Command::new("git")
                .args(["check-ignore", "-q", path])
                .current_dir(dir.path())
                .status()
                .unwrap()
                .success()
        };
        assert!(!ignored(".alien/signing.json"), "the pin is committed");
        assert!(ignored(".alien/build.json"), "build output stays ignored");

        let other = RepoSigning {
            fingerprint: format!("SHA256:{}", "b".repeat(64)),
            project: "my-app".to_string(),
        };
        other
            .write(dir.path())
            .expect_err("a different pin is never written over the first");
    }
}
