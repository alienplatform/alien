//! Signing a release: deciding whether this release can be signed here,
//! turning signing on for a project (only `alien devices init` does), filing
//! a trust request for a device the list doesn't hold yet, and building the
//! `release/v2` statements.
//!
//! A release that can't be signed goes out unsigned with one line saying
//! why. The caller fails it only when the channel has protected deployments,
//! because those would refuse it. A project that never ran `alien devices
//! init` has no `.alien/signing.json`: its releases are unsigned and make no
//! signing request.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;

use alien_cli_common::setup_templates::{
    protected_stack_settings, render_setup_template, RenderedSetupTemplate, SetupHostingOptions,
    SetupRenderOptions,
};
use alien_cloudformation::RegistrationMode;
use alien_core::release_statement::{
    check_deployer_env_mappings, release_body, signed_stack, ReleaseOptions,
};
use alien_core::{Platform, SetupPins, Stack, StackSigning};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_signing::{
    display_fingerprint, fingerprint, Envelope, Kind, RecoveryCode, ReleaseBody, Role,
    SetupTemplate, SignedStatement, Signer, SignerChain, SignerKind, SignerListBody,
};
use base64::Engine as _;
use chrono::Utc;

use super::api::{ListPublished, SigningApi};
use super::device::Backend;
use super::{
    ci_key, list_expiry, pairing_code, signer_with_key, verified_chain, RepoSigning, SigningHome,
    StatementSigner, LIST_WARNING_DAYS, REPO_SIGNING_FILE,
};
use crate::error::{ErrorData, Result};

/// Digests of the enforcer builds this CLI build ships with, by component
/// name. The CLI release workflow fills the file before it builds; a build
/// from source has none, so its releases pin no enforcer builds.
const ENFORCER_DIGESTS: &str = include_str!("../../enforcer-digests.json");

/// Who signs this release, verified against the project's list.
pub struct ReleaseSigner {
    /// The key.
    pub signer: StatementSigner,
    /// The list it was verified on.
    pub chain: SignerChain,
}

/// What `alien release` does about signing.
pub enum SigningPlan {
    /// Sign with this key.
    Sign(Box<ReleaseSigner>),
    /// Release unsigned, for this reason.
    Unsigned(String),
    /// Release unsigned: this device asked to be trusted, and the request,
    /// with its pairing code, is already on screen.
    Requested(String),
}

/// Why a repository without `.alien/signing.json` releases unsigned.
pub fn signing_off_hint() -> String {
    format!(
        "this repo has no {REPO_SIGNING_FILE}; `alien devices init` turns signing on for the project"
    )
}

/// Decide how this release is signed. `interactive` is a person at a
/// terminal: only then is a trust request filed, and only with a hardware
/// key. A repository without the signing pin never reaches the control plane.
pub async fn plan(
    api: &SigningApi,
    dir: &Path,
    project: &str,
    interactive: bool,
) -> Result<SigningPlan> {
    let home = SigningHome::current()?;
    let ci = ci_key()?;
    let Some(repo) = RepoSigning::read(dir)? else {
        return Ok(unsigned(signing_off_hint()));
    };
    let chain = match verified_chain(api, &repo).await {
        Ok(chain) => chain,
        Err(error) => return Ok(unsigned(error.message)),
    };
    let list = chain.current();
    if list.is_expired(Utc::now()) {
        return Ok(unsigned(format!(
            "the device list expired on {}; an admin must run `alien approve` to refresh it",
            list.not_after.format("%Y-%m-%d")
        )));
    }
    let signer = if let Some(key) = ci {
        let Some(signer) = signer_with_key(list, &key.public_key()) else {
            return Ok(unsigned(format!(
                "the CI key isn't on the device list for {}",
                repo.project
            )));
        };
        let left = (list.not_after - Utc::now()).num_days();
        if left < LIST_WARNING_DAYS {
            eprintln!(
                "! The device list for {} expires in {left} days, on {}. CI can't refresh it; an admin must run `alien approve`.",
                repo.project,
                list.not_after.format("%Y-%m-%d")
            );
        }
        StatementSigner::Key {
            id: signer.id.clone(),
            key,
        }
    } else {
        let device = if interactive {
            home.device_or_create()?
        } else {
            home.device()?
        };
        let Some((record, key)) = device else {
            return Ok(unsigned("this device has no hardware key to sign with"));
        };
        let Some(signer) = signer_with_key(list, &key.public_key) else {
            if !interactive {
                return Ok(unsigned(format!(
                    "this device isn't trusted for {} yet",
                    repo.project
                )));
            }
            request_trust(api, &chain, &record, &key.public_key).await?;
            return Ok(SigningPlan::Requested(format!(
                "this device isn't trusted for {} yet",
                repo.project
            )));
        };
        StatementSigner::Device {
            id: signer.id.clone(),
            key,
        }
    };
    if !super::holds(list, &signer, Role::Release) {
        return Ok(unsigned(format!(
            "{} may not release for {}",
            signer.id(),
            repo.project
        )));
    }
    Ok(SigningPlan::Sign(Box::new(ReleaseSigner { signer, chain })))
}

fn unsigned(reason: impl Into<String>) -> SigningPlan {
    SigningPlan::Unsigned(reason.into())
}

/// What `alien devices init` did.
pub enum Enabled {
    /// Signing is on: version 1 of the list is stored and pinned in the
    /// repository.
    On {
        /// The fingerprint of version 1, which is the pin.
        fingerprint: String,
    },
    /// Someone else turned it on first; this is who.
    AlreadyOn {
        /// The label of version 1's first admin.
        by: String,
    },
}

/// Turn signing on for `project`: create list version 1 with this device as
/// admin and a fresh recovery key, show the recovery code once and have the
/// admin type its last four characters back, store the list and pin it in
/// the repository. Nothing is published before the code is confirmed. `None`
/// when the control plane has no signer list route: it can't have protected
/// installs either.
pub async fn enable(api: &SigningApi, dir: &Path, project: &str) -> Result<Option<Enabled>> {
    // Before the hardware key asks for a PIN: a control plane that can't store signer lists
    // makes signing impossible whatever the device does.
    let Some(lists) = api.signer_lists_if_supported(project).await? else {
        return Ok(None);
    };
    // Someone already turned signing on: say who (the first admin of
    // version 1) without showing a code that would never be used.
    if let Some(first) = lists.first() {
        return Ok(Some(Enabled::AlreadyOn {
            by: first_admin(first),
        }));
    }
    let Some((record, key)) = SigningHome::current()?.device_or_create()? else {
        return Err(AlienError::new(ErrorData::SigningFailed {
            message: "This device has no hardware key to sign with".to_string(),
        }));
    };
    let device = StatementSigner::Device {
        id: record.signer_id.clone(),
        key,
    };
    let code = RecoveryCode::generate().context(ErrorData::SigningFailed {
        message: "Could not create the recovery code".to_string(),
    })?;
    let recovery = code.key(project).context(ErrorData::SigningFailed {
        message: "Could not derive the recovery key".to_string(),
    })?;
    let body = SignerListBody {
        previous: None,
        admin_threshold: 1,
        signers: vec![
            Signer {
                id: record.signer_id.clone(),
                key: device.public_key().as_str().to_string(),
                kind: SignerKind::Device,
                roles: vec![Role::Release, Role::Operate, Role::Admin],
                label: record.label.clone(),
            },
            Signer {
                id: RECOVERY_SIGNER.to_string(),
                key: recovery.public_key().as_str().to_string(),
                kind: SignerKind::Recovery,
                roles: vec![Role::Admin],
                label: None,
            },
        ],
        deny: Vec::new(),
        recovered: false,
    };
    let envelope = Envelope {
        kind: Kind::SignerList.as_str().to_string(),
        project: project.to_string(),
        install: None,
        sequence: 1,
        not_after: Some(list_expiry(Utc::now())),
        signer_list_version: 1,
        body,
    };
    // Version 1 only has to be signed by one of its own admins. The
    // recovery key signs it, so turning signing on costs no touch beyond the
    // one that signs the release.
    let recovery_signer = StatementSigner::Key {
        id: RECOVERY_SIGNER.to_string(),
        key: recovery,
    };
    let statement = super::sign_envelope(&envelope, &recovery_signer)?;
    let opened = statement.open().context(ErrorData::SigningFailed {
        message: "Could not read the new device list".to_string(),
    })?;
    SignerChain::from_statements(
        &fingerprint(&opened.digest),
        std::slice::from_ref(&statement),
        Utc::now(),
    )
    .context(ErrorData::SigningFailed {
        message: "The new device list does not verify".to_string(),
    })?;
    let this = match record.backend {
        Backend::SecureEnclave => "this Mac",
        Backend::Tpm | Backend::Yubikey => "this machine",
    };
    println!();
    println!("  Recovery code (shown once — save it in your password manager):");
    println!("    {}", code.display());
    println!("  It restores signing if {this} is lost.");
    confirm_recovery_code(&code)?;
    match api.publish_signer_list(project, &statement).await? {
        ListPublished::Stored => {}
        ListPublished::AlreadyOn { enabled_by } => {
            println!("  That recovery code was not used; discard it.");
            return Ok(Some(Enabled::AlreadyOn { by: enabled_by }));
        }
    }
    let repo = RepoSigning {
        fingerprint: fingerprint(&opened.digest),
        project: project.to_string(),
    };
    repo.write(dir)?;
    println!();
    println!(
        "  Signing is now on for {project}. {} is its first trusted device.",
        capitalize(this)
    );
    println!("  Wrote {REPO_SIGNING_FILE} — commit it.");
    println!("  Fingerprint {}", display_fingerprint(&repo.fingerprint));
    println!();
    Ok(Some(Enabled::On {
        fingerprint: repo.fingerprint,
    }))
}

/// How many times the admin may mistype the last four characters.
const CONFIRMATION_TRIES: usize = 3;

/// Ask for the last four characters of `code`, so signing is never turned on
/// for a code nobody saved.
fn confirm_recovery_code(code: &RecoveryCode) -> Result<()> {
    for _ in 0..CONFIRMATION_TRIES {
        print!("  Type the last 4 characters of the code to confirm it is saved ▸ ");
        std::io::stdout()
            .flush()
            .into_alien_error()
            .context(ErrorData::CliInteractionFailed {
                message: "Failed to ask for the recovery code confirmation".to_string(),
            })?;
        let mut line = String::new();
        let read = std::io::stdin()
            .read_line(&mut line)
            .into_alien_error()
            .context(ErrorData::CliInteractionFailed {
                message: "Failed to read the confirmation".to_string(),
            })?;
        if read == 0 {
            break;
        }
        if code.ends_with(&line) {
            return Ok(());
        }
        println!("  Those aren't the last 4 characters of the code above.");
    }
    println!();
    Err(AlienError::new(ErrorData::CliInteractionFailed {
        message: "The recovery code wasn't confirmed, so signing was not turned on. Run `alien devices init` again.".to_string(),
    }))
}

/// Who turned signing on: the label (or id) of version 1's device admin.
fn first_admin(first: &SignedStatement) -> String {
    first
        .open()
        .and_then(|opened| opened.body::<SignerListBody>())
        .ok()
        .and_then(|body| {
            body.signers
                .into_iter()
                .find(|signer| {
                    signer.kind == SignerKind::Device && signer.roles.contains(&Role::Admin)
                })
                .map(|signer| signer.label.unwrap_or(signer.id))
        })
        .unwrap_or_else(|| "another admin".to_string())
}

/// Signer id of the recovery key on every list.
pub const RECOVERY_SIGNER: &str = "recovery";

fn capitalize(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// File a request to add this device to the list, or show the one already
/// waiting, with the pairing code the admin must type.
async fn request_trust(
    api: &SigningApi,
    chain: &SignerChain,
    record: &super::DeviceRecord,
    key: &alien_signing::PublicKey,
) -> Result<()> {
    let project = chain.project().to_string();
    let list = chain.current();
    // What this device asks for, built here: a waiting request is reused
    // only if it asks for exactly this, so the control plane can't put other
    // roles, a kind or another change behind this device's pairing code.
    let mut id = record.signer_id.clone();
    let mut suffix = 2;
    while list.signer(&id).is_some() {
        id = format!("{}-{suffix}", record.signer_id);
        suffix += 1;
    }
    let signer = Signer {
        id,
        key: key.as_str().to_string(),
        kind: SignerKind::Device,
        roles: vec![Role::Release],
        label: record.label.clone(),
    };
    let mut body = list.body.clone();
    body.previous = Some(list.digest.clone());
    body.recovered = false;
    body.signers.push(signer.clone());

    let pending = api.pending(&project).await?;
    let waiting = pending.into_iter().find(|change| {
        added_signers(&list.body, &change.body)
            .iter()
            .any(|added| added.key == key.as_str())
    });
    let change = match waiting {
        Some(change) if change.based_on_version == list.version && change.body == body => change,
        Some(change) => {
            return Err(AlienError::new(ErrorData::SigningFailed {
                message: format!(
                    "Waiting change {} adds this device but isn't the request this device makes (one device with role release, based on version {}). Ask an admin to reject it in the dashboard, then release again.",
                    change.id, list.version
                ),
            }))
        }
        None => {
            let summary = format!("{} asks to: release", signer.id);
            api.create_pending(&project, &summary, list.version, &body)
                .await?
        }
    };
    let admins: Vec<String> = list
        .body
        .signers
        .iter()
        .filter(|signer| signer.roles.contains(&Role::Admin) && signer.kind == SignerKind::Device)
        .map(|signer| signer.label.clone().unwrap_or_else(|| signer.id.clone()))
        .collect();
    println!("! Not signed: this device isn't trusted for {project} yet.");
    println!(
        "  Asked admins ({}) to trust {}. Send them this code over Slack or in person:",
        admins.join(", "),
        signer.id
    );
    println!("    {}", pairing_code(&signer, &change.id));
    Ok(())
}

/// Signers `next` has that `current` doesn't, by key.
pub fn added_signers<'a>(current: &SignerListBody, next: &'a SignerListBody) -> Vec<&'a Signer> {
    next.signers
        .iter()
        .filter(|signer| !current.signers.iter().any(|old| old.key == signer.key))
        .collect()
}

/// Digests of the enforcer builds compiled into this CLI.
pub fn enforcer_digests() -> Result<BTreeMap<String, String>> {
    parse_enforcer_digests(ENFORCER_DIGESTS)
}

fn parse_enforcer_digests(text: &str) -> Result<BTreeMap<String, String>> {
    let table: BTreeMap<String, String> =
        serde_json::from_str(text)
            .into_alien_error()
            .context(ErrorData::ConfigurationError {
                message: "The enforcer digest table in this CLI build is not a JSON object"
                    .to_string(),
            })?;
    for (name, digest) in &table {
        if !alien_signing::release::COMPONENTS.contains(&name.as_str())
            || !alien_signing::release::is_sha256_digest(digest)
        {
            return Err(AlienError::new(ErrorData::ConfigurationError {
                message: format!(
                    "The enforcer digest table in this CLI build has '{name}': '{digest}'"
                ),
            }));
        }
    }
    Ok(table)
}

/// Setup templates of `platform`, from [`SETUP_TEMPLATES`](alien_signing::release::SETUP_TEMPLATES).
fn platform_templates(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Aws => &["cloudformation", "terraform-aws"],
        Platform::Gcp => &["terraform-gcp"],
        Platform::Azure => &["terraform-azure"],
        Platform::Kubernetes => &["helm"],
        // Machines join with `join`, and local and test stacks have no setup.
        Platform::Machines | Platform::Local | Platform::Test => &[],
    }
}

/// The setup templates a protected install of `platform` uses, with the
/// parameters a customer may choose. They are rendered with the function
/// and hosting options the control plane renders with (`hosting` comes from
/// it, `pins` are this build's own); the release signs their digests and
/// uploads these bytes, which are the ones protected installs are given.
/// This is the one place the release renders setup.
pub async fn setup_templates(
    stack: &Stack,
    platform: Platform,
    signing: &StackSigning,
    hosting: &SetupHostingOptions,
    pins: SetupPins,
) -> Result<BTreeMap<String, RenderedSetupTemplate>> {
    let cloudformation = crate::commands::render::cloudformation_registry();
    let terraform = crate::commands::render::terraform_registry();
    let helm = alien_helm::HelmRegistry::built_in();
    let options = SetupRenderOptions::builder()
        .signing(signing)
        .cloudformation_registry(&cloudformation)
        .terraform_registry(&terraform)
        .helm_registry(&helm)
        .hosting(hosting.clone())
        .pins(pins)
        .build();
    let mut templates = BTreeMap::new();
    for &name in platform_templates(platform) {
        let settings =
            protected_stack_settings(name, stack, signing).context(ErrorData::SigningFailed {
                message: format!("Could not choose the settings of the {name} setup template"),
            })?;
        let rendered = render_setup_template(name, stack, &settings, &options)
            .await
            .context(ErrorData::SigningFailed {
                message: format!("Could not render the {name} setup template"),
            })?;
        templates.insert(name.to_string(), rendered);
    }
    Ok(templates)
}

/// The digest and allow-list the release signs for each rendered template.
pub fn signed_setup(
    rendered: &BTreeMap<String, RenderedSetupTemplate>,
) -> BTreeMap<String, SetupTemplate> {
    rendered
        .iter()
        .map(|(name, template)| (name.clone(), template.setup_template()))
        .collect()
}

/// The rendered bytes of each template, base64-encoded as the release
/// uploads them beside its signed statements.
pub fn setup_upload(
    rendered: &BTreeMap<String, RenderedSetupTemplate>,
) -> std::collections::HashMap<String, String> {
    rendered
        .iter()
        .map(|(name, template)| {
            (
                name.clone(),
                base64::engine::general_purpose::STANDARD.encode(&template.bytes),
            )
        })
        .collect()
}

/// One line saying what setup is rendered with, for the signing summary.
pub fn hosting_summary(hosting: &SetupHostingOptions) -> String {
    let registration = match &hosting.registration {
        Some(
            RegistrationMode::CustomResource { .. }
            | RegistrationMode::RegionalCustomResource { .. },
        ) => "custom resource",
        Some(RegistrationMode::OutputsFallback) => "stack outputs",
        Some(RegistrationMode::Both { .. } | RegistrationMode::RegionalBoth { .. }) => {
            "custom resource and stack outputs"
        }
        None => "`register --import terraform` (no CloudFormation setup)",
    };
    let management = if hosting
        .management_trust
        .allowed_managing_role_arns
        .is_empty()
    {
        "any managing role".to_string()
    } else {
        hosting
            .management_trust
            .allowed_managing_role_arns
            .join(", ")
    };
    format!(
        "Setup as the control plane hosts it · registers through {registration} · managed by {management}{}",
        if hosting.management_trust.require_external_id {
            " with an external id"
        } else {
            ""
        }
    )
}

/// The `release/v2` body for one platform's pushed stack.
pub fn body(
    platform: Platform,
    stack: &Stack,
    channel: &str,
    components: &BTreeMap<String, String>,
    setup: BTreeMap<String, SetupTemplate>,
) -> Result<ReleaseBody> {
    check_deployer_env_mappings(stack, platform).context(ErrorData::SigningFailed {
        message: format!("Can't sign the {} stack", platform.as_str()),
    })?;
    release_body(
        platform,
        stack,
        &ReleaseOptions {
            channel: channel.to_string(),
            configurable_env: Vec::new(),
            configurable_inputs: Vec::new(),
            components: components.clone(),
            setup,
        },
    )
    .context(ErrorData::SigningFailed {
        message: format!("Can't sign the {} stack", platform.as_str()),
    })
}

/// Sign every platform's body with one gesture, then check each statement
/// verifies on the list and reads back as the stack it was built from.
pub fn sign_all(
    signing: &ReleaseSigner,
    bodies: Vec<(Platform, Stack, ReleaseBody)>,
) -> Result<Vec<(Platform, SignedStatement)>> {
    let sequence = super::new_sequence();
    let mut payloads = Vec::new();
    for (_, _, body) in &bodies {
        let mut envelope = super::envelope(Kind::Release, &signing.chain, None, body);
        envelope.sequence = sequence;
        payloads.push(envelope.to_payload().context(ErrorData::SigningFailed {
            message: "The release statement could not be encoded".to_string(),
        })?);
    }
    let signatures = match &signing.signer {
        StatementSigner::Device { key, .. } => key.sign_all(&payloads)?,
        StatementSigner::Key { key, .. } => {
            payloads.iter().map(|payload| key.sign(payload)).collect()
        }
    };
    let list = signing.chain.current();
    let mut statements = Vec::new();
    for ((platform, stack, _), (payload, sig)) in
        bodies.into_iter().zip(payloads.into_iter().zip(signatures))
    {
        let mut statement = SignedStatement::new(&payload);
        statement.signatures.push(alien_signing::Signature {
            signer: signing.signer.id().to_string(),
            sig,
        });
        let opened = statement.open().context(ErrorData::SigningFailed {
            message: "The signed release could not be read back".to_string(),
        })?;
        list.check_signatures(&opened, Role::Release)
            .context(ErrorData::SigningFailed {
                message: "The release signature does not verify on the device list".to_string(),
            })?;
        let verified: ReleaseBody = opened.body().context(ErrorData::SigningFailed {
            message: "The signed release body does not read back".to_string(),
        })?;
        let carried = signed_stack(&verified, platform).context(ErrorData::SigningFailed {
            message: format!("The signed {} stack is refused", platform.as_str()),
        })?;
        if carried != stack {
            return Err(AlienError::new(ErrorData::SigningFailed {
                message: format!(
                    "The signed {} stack does not read back as the pushed stack",
                    platform.as_str()
                ),
            }));
        }
        statements.push((platform, statement));
    }
    Ok(statements)
}

/// Lines describing how `next` changes the enforcer builds `previous` ran
/// with, one per component.
pub fn component_changes(
    previous: &BTreeMap<String, String>,
    next: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut names: Vec<&String> = previous.keys().chain(next.keys()).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| match (previous.get(name), next.get(name)) {
            (Some(old), Some(new)) if old != new => {
                Some(format!("{name}: {} → {}", short(old), short(new)))
            }
            (Some(old), None) => Some(format!("{name}: {} → none", short(old))),
            (None, Some(new)) => Some(format!("{name}: none → {}", short(new))),
            _ => None,
        })
        .collect()
}

fn short(digest: &str) -> String {
    digest.chars().take(19).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest as _;

    const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn storage_stack() -> Stack {
        serde_json::from_value(serde_json::json!({
            "id": "minimal",
            "resources": {
                "assets": {
                    "config": {
                        "corsAllowedOrigins": [], "id": "assets", "lifecycleRules": [],
                        "publicRead": false, "type": "storage", "versioning": false
                    },
                    "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
                }
            },
            "permissions": { "profiles": { "execution": {} }, "management": "auto" }
        }))
        .unwrap()
    }

    fn pins() -> SetupPins {
        SetupPins {
            aws_publisher: Some("111122223333".to_string()),
            aws_verifier_image: Some(format!("verifier@sha256:{}", "5d".repeat(32))),
            gcp_verifier_image: Some(format!("gcr.example/verifier@sha256:{}", "6e".repeat(32))),
            azure_verifier_image: Some(format!(
                "registry.example/verifier@sha256:{}",
                "7f".repeat(32)
            )),
            operator_digest: Some(format!("sha256:{}", "0b".repeat(32))),
        }
    }

    fn hosting(require_external_id: bool) -> SetupHostingOptions {
        serde_json::from_value(serde_json::json!({
            "registration": { "mode": "outputsFallback" },
            "managementTrust": {
                "requireExternalId": require_external_id,
                "allowedManagingRoleArns": ["arn:aws:iam::999988887777:role/manager"]
            }
        }))
        .unwrap()
    }

    /// What a release signs for each platform is the digest of the bytes the
    /// shared renderer gives the control plane for the same stack, signing
    /// settings and hosting.
    #[tokio::test]
    async fn release_digests_are_of_the_templates_the_control_plane_renders() {
        let stack = storage_stack();
        let signing = StackSigning {
            fingerprint: format!("SHA256:{}", "9f".repeat(32)),
            channel: "production".to_string(),
        };
        let cloudformation = crate::commands::render::cloudformation_registry();
        let terraform = crate::commands::render::terraform_registry();
        let helm = alien_helm::HelmRegistry::built_in();
        let all = alien_cli_common::setup_templates::render_setup_templates(
            &stack,
            SetupRenderOptions::builder()
                .signing(&signing)
                .cloudformation_registry(&cloudformation)
                .terraform_registry(&terraform)
                .helm_registry(&helm)
                .hosting(hosting(true))
                .pins(pins())
                .build(),
        )
        .await
        .unwrap();
        let mut signed = BTreeMap::new();
        for platform in [
            Platform::Aws,
            Platform::Gcp,
            Platform::Azure,
            Platform::Kubernetes,
        ] {
            signed.extend(
                setup_templates(&stack, platform, &signing, &hosting(true), pins())
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(signed, all);
        let mut names: Vec<&str> = alien_signing::release::SETUP_TEMPLATES.to_vec();
        names.sort();
        assert_eq!(signed.keys().map(String::as_str).collect::<Vec<_>>(), names);

        // Hosting reaches the bytes: other management trust is another template.
        let other = setup_templates(&stack, Platform::Aws, &signing, &hosting(false), pins())
            .await
            .unwrap();
        assert_ne!(
            other["cloudformation"].bytes,
            signed["cloudformation"].bytes
        );

        // What is uploaded decodes to the bytes whose digests are signed.
        let upload = setup_upload(&signed);
        for (name, template) in signed_setup(&signed) {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&upload[&name])
                .unwrap();
            assert_eq!(
                format!("sha256:{}", hex::encode(sha2::Sha256::digest(&bytes))),
                template.sha256,
                "{name}"
            );
        }
    }

    const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn the_source_build_pins_no_enforcer_builds() {
        assert!(enforcer_digests().unwrap().is_empty());
        let table = parse_enforcer_digests(&format!(r#"{{"operator": "{A}"}}"#)).unwrap();
        assert_eq!(table["operator"], A);
        parse_enforcer_digests(&format!(r#"{{"someone-else": "{A}"}}"#))
            .expect_err("an unknown component is refused");
        parse_enforcer_digests(r#"{"operator": "latest"}"#).expect_err("a tag is refused");
    }

    #[test]
    fn component_changes_name_each_moved_build() {
        let previous = BTreeMap::from([
            ("operator".to_string(), A.to_string()),
            ("machine-agent/linux-x86_64".to_string(), A.to_string()),
        ]);
        let next = BTreeMap::from([
            ("operator".to_string(), B.to_string()),
            ("machine-agent/linux-x86_64".to_string(), A.to_string()),
            ("machine-agent/linux-aarch64".to_string(), B.to_string()),
        ]);
        assert_eq!(
            component_changes(&previous, &next),
            vec![
                "machine-agent/linux-aarch64: none → sha256:bbbbbbbbbbbb".to_string(),
                "operator: sha256:aaaaaaaaaaaa → sha256:bbbbbbbbbbbb".to_string(),
            ]
        );
        assert!(component_changes(&next, &next).is_empty());
    }

    /// A repository without the signing pin releases unsigned and never
    /// reaches the control plane: no signer list route is read, whatever
    /// the terminal or the device. (The mock server fails the test if any
    /// request arrives.)
    #[tokio::test]
    async fn a_repository_without_the_pin_releases_unsigned_without_any_signing_request() {
        let server = httpmock::MockServer::start_async().await;
        let any = server
            .mock_async(|when, then| {
                when.any_request();
                then.status(500).body("must not be called");
            })
            .await;
        let api = SigningApi::new(reqwest::Client::new(), server.base_url());
        let dir = tempfile::tempdir().unwrap();

        for interactive in [true, false] {
            match plan(&api, dir.path(), "shop", interactive).await.unwrap() {
                SigningPlan::Unsigned(reason) => assert_eq!(
                    reason,
                    "this repo has no .alien/signing.json; `alien devices init` turns signing on for the project"
                ),
                _ => panic!("expected an unsigned release"),
            }
        }

        any.assert_hits_async(0).await;
        assert!(RepoSigning::read(dir.path()).unwrap().is_none());
    }

    /// A control plane from before signed releases has no signer list route.
    /// `alien devices init` finds that out before the device is touched, so
    /// nobody is asked for a hardware key's PIN for signing that can't work,
    /// and writes no repository pin. (This test reaches no hardware: the
    /// decision comes first.)
    #[tokio::test]
    async fn init_on_a_control_plane_without_signing_turns_nothing_on_before_any_pin() {
        let server = httpmock::MockServer::start_async().await;
        let lists = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v1/projects/shop/signer-lists");
                then.status(404).body("404 Not Found");
            })
            .await;
        let api = SigningApi::new(reqwest::Client::new(), server.base_url());
        let dir = tempfile::tempdir().unwrap();

        assert!(enable(&api, dir.path(), "shop").await.unwrap().is_none());

        lists.assert_async().await;
        assert!(RepoSigning::read(dir.path()).unwrap().is_none());

        // Any other failure still stops it.
        let server = httpmock::MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.path("/v1/projects/shop/signer-lists");
                then.status(500).body("boom");
            })
            .await;
        let api = SigningApi::new(reqwest::Client::new(), server.base_url());
        assert!(enable(&api, dir.path(), "shop").await.is_err());
    }
}
