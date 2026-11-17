//! `verify`'s check of the installed setup template: render it again.
//!
//! A protected template records the renderer version and the hosting inputs it was rendered
//! with ([`SetupRecord`]). `verify` renders the template again from each signed release's stack,
//! the install's own signing settings and those recorded inputs, and compares bytes. Nothing the
//! vendor's control plane sent decides what the right template is except the signed stack; the
//! pins that come with the renderer build (verifier image, machine-image publisher, the Operator
//! image a chart starts from) are recomputed by rendering with this build, so a changed pin is a
//! changed byte.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use alien_cli_common::setup_templates::{
    render_setup_template, SetupHostingOptions, SetupRecord, SetupRenderOptions, RENDERER_VERSION,
};
use alien_core::release_statement::signed_stack;
use alien_core::{Platform, StackSigning};
use alien_signing::{ReleaseBody, SigningSettings, Trust};
use chrono::{DateTime, Utc};
use semver::Version;

use super::verify::{line, running_release, Content, Installed, Line, Mark, Signed};

/// The emitter registries `verify` renders with: the public ones. This is the one place that
/// says which resource types `verify` can render; a build with more emitters adds them here.
fn public_registries() -> (
    alien_cloudformation::CfRegistry,
    alien_terraform::TfRegistry,
    alien_helm::HelmRegistry,
) {
    (
        alien_cloudformation::CfRegistry::built_in(),
        alien_terraform::TfRegistry::built_in(),
        alien_helm::HelmRegistry::built_in(),
    )
}

/// The pins that come with this renderer build. Tests render with their own, since a source build
/// has none.
fn pins() -> alien_core::SetupPins {
    #[cfg(test)]
    {
        test_pins()
    }
    #[cfg(not(test))]
    {
        alien_core::SetupPins::compiled_in()
    }
}

/// The pins tests render and verify with.
#[cfg(test)]
pub(crate) fn test_pins() -> alien_core::SetupPins {
    alien_core::SetupPins {
        aws_publisher: Some("111122223333".to_string()),
        aws_verifier_image: Some(format!("verifier@sha256:{}", "5d".repeat(32))),
        gcp_verifier_image: Some(format!("gcr.example/verifier@sha256:{}", "6e".repeat(32))),
        azure_verifier_image: Some(format!(
            "registry.example/verifier@sha256:{}",
            "7f".repeat(32)
        )),
        operator_digest: Some(format!("sha256:{}", "ef".repeat(32))),
    }
}

/// The resource types a failed render had no emitter for, from the error chain.
fn missing_emitters(
    error: &alien_error::AlienError<alien_cli_common::setup_templates::ErrorData>,
) -> Vec<String> {
    let mut found = Vec::new();
    let mut current = error.source.as_deref();
    while let Some(error) = current {
        if error.code == "IMPORT_REGISTRATION_MISSING" {
            let resource = error.context.as_ref().and_then(|context| {
                context
                    .get("resourceType")
                    .or_else(|| context.get("resource_type"))
            });
            if let Some(resource) = resource {
                let text = resource
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| resource.to_string());
                if !found.contains(&text) {
                    found.push(text);
                }
            }
        }
        current = error.source.as_deref();
    }
    found
}

/// The release an installed template was rendered for.
pub(crate) struct Matched {
    /// The control plane's id for the release.
    pub id: String,
    /// The release's channel.
    pub channel: String,
    /// What customers may choose.
    pub allowed: Vec<String>,
    /// Set when the release's signer has left the list since.
    pub signed_by_removed: Option<String>,
}

/// What a release candidate is: a statement for this platform and channel that has the template.
struct Candidate {
    id: String,
    sequence: u64,
    body: ReleaseBody,
    min: Version,
    /// Why it counts although its signer left the list.
    removed_signer: Option<String>,
}

/// The `alien-deploy` a customer runs to check a template rendered by `version`: a copy-paste
/// command that downloads it from the public release and checks its attestation.
pub(crate) fn run_this_version(version: &str) -> String {
    format!(
        "download alien-deploy {version} from the public release and check it before use: \
         `gh release download v{version} --repo alienplatform/alien --pattern 'alien-deploy*' --dir alien-deploy-{version} \
         && gh release verify-asset v{version} alien-deploy-{version}/<the file for your system> --repo alienplatform/alien`, \
         then run `verify` with it"
    )
}

/// The template's renderer line, its hosting lines and the template line. The match is the
/// release the template is rendered for, if any.
pub(crate) async fn check_template(
    trust: &Trust,
    settings: &SigningSettings,
    signed: &Signed,
    installed: &Installed,
    now: DateTime<Utc>,
) -> (Vec<Line>, Option<Matched>) {
    let digest = installed.content.digest();
    let mut lines = Vec::new();
    let record = match installed.content.record(installed.template) {
        Ok(record) => record,
        Err(error) => {
            lines.push(line(
                "Renderer",
                format!("not recorded in the install, so it can't be rendered again: {error}"),
                Mark::Fail,
            ));
            return (lines, None);
        }
    };
    let recorded = match Version::parse(&record.renderer) {
        Ok(version) => version,
        Err(error) => {
            lines.push(line(
                "Renderer",
                format!(
                    "the recorded version '{}' is not a version: {error}",
                    record.renderer
                ),
                Mark::Fail,
            ));
            return (lines, None);
        }
    };
    let candidates = candidates(trust, settings, signed, installed.template, now);
    // No release to hold it to: the template can't match any.
    let Some(newest) = candidates.first() else {
        lines.push(line(
            "Renderer",
            format!("{} (as recorded)", record.renderer),
            Mark::Info,
        ));
        lines.extend(hosting_lines(installed.template, &record));
        lines.push(line(
            "Template",
            format!(
                "{digest} is not the {} template of any release signed for {}",
                installed.template, settings.channel
            ),
            Mark::Fail,
        ));
        return (lines, None);
    };
    // No downgrade: a template an older renderer made isn't accepted once a release signs a
    // newer minimum.
    if recorded < newest.min {
        lines.push(line(
            "Renderer",
            format!(
                "{recorded} is older than {}, the oldest renderer the newest signed release ({}) accepts",
                newest.min, newest.id
            ),
            Mark::Fail,
        ));
        lines.extend(hosting_lines(installed.template, &record));
        return (lines, None);
    }
    if record.renderer != RENDERER_VERSION {
        lines.push(line(
            "Renderer",
            format!(
                "the template was rendered by alien {} but this is alien-deploy {RENDERER_VERSION}; \
                 it is not checked with another version: {}",
                record.renderer,
                run_this_version(&record.renderer)
            ),
            Mark::Fail,
        ));
        lines.extend(hosting_lines(installed.template, &record));
        return (lines, None);
    }
    lines.push(line(
        "Renderer",
        format!("alien {} (this build)", record.renderer),
        Mark::Pass,
    ));
    lines.extend(hosting_lines(installed.template, &record));

    let signing = StackSigning {
        fingerprint: settings.fingerprint.clone(),
        channel: settings.channel.clone(),
    };
    let mut first_difference = None;
    for candidate in candidates.iter().filter(|c| c.min <= recorded) {
        match rendered(candidate, signed.platform, &signing, &record, installed).await {
            Ok(None) => {
                let channel = candidate.body.channel.clone();
                let matched = Matched {
                    id: candidate.id.clone(),
                    channel: channel.clone(),
                    allowed: candidate.body.setup[installed.template].allowed.clone(),
                    signed_by_removed: candidate.removed_signer.clone(),
                };
                lines.push(match &candidate.removed_signer {
                    None => line(
                        "Template",
                        format!(
                            "rendered again from release {} · {channel} · {digest}",
                            candidate.id
                        ),
                        Mark::Pass,
                    ),
                    // A release whose signer has since left the list still verifies against
                    // the list it names and keeps running until the next release (only the
                    // deny-list stops it), and so does the template it came with.
                    Some(by) => line(
                        "Template",
                        format!(
                            "rendered again from release {} · {channel} · {digest} · signed by {by}, no longer on the list",
                            candidate.id
                        ),
                        Mark::Info,
                    ),
                });
                return (lines, Some(matched));
            }
            Ok(Some(difference)) => {
                first_difference.get_or_insert((candidate.id.clone(), difference));
            }
            Err(reason) => {
                first_difference.get_or_insert((candidate.id.clone(), reason));
            }
        }
    }
    let detail = match first_difference {
        Some((id, difference)) => format!("against release {id}: {difference}"),
        None => "no release accepts this renderer version".to_string(),
    };
    lines.push(line(
        "Template",
        format!(
            "{digest} is not what any release signed for {} renders to ({detail})",
            settings.channel
        ),
        Mark::Fail,
    ));
    (lines, None)
}

/// Releases that could have produced a template of this install, newest first: signed for the
/// install's platform and channel, with this template, either verified by the signer list
/// now or by the list they name with their signer since removed.
fn candidates(
    trust: &Trust,
    settings: &SigningSettings,
    signed: &Signed,
    template: &str,
    now: DateTime<Utc>,
) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = signed
        .releases
        .iter()
        .filter_map(|(id, statement)| {
            let opened = statement.open().ok()?;
            let body: ReleaseBody = opened.body().ok()?;
            if body.platform != signed.platform || body.channel != settings.channel {
                return None;
            }
            body.setup.get(template)?;
            let min = Version::parse(&body.setup_renderer.as_ref()?.min).ok()?;
            let removed_signer = match trust.verify_release(statement, now) {
                Ok(_) => None,
                Err(_) => {
                    let (_, signers, _) =
                        running_release(trust, settings, &signed.releases, &opened.digest, now)
                            .ok()?;
                    Some(signers.join(", "))
                }
            };
            Some(Candidate {
                id: id.clone(),
                sequence: opened.sequence,
                body,
                min,
                removed_signer,
            })
        })
        .collect();
    found.sort_by_key(|candidate| Reverse(candidate.sequence));
    found
}

/// Render the template `candidate`'s signed stack gives with the install's signing settings and
/// the recorded hosting inputs, and compare it with the installed one: none when equal, the
/// first difference otherwise.
async fn rendered(
    candidate: &Candidate,
    platform: &str,
    signing: &StackSigning,
    record: &SetupRecord,
    installed: &Installed,
) -> Result<Option<String>, String> {
    let platform: Platform = platform
        .parse()
        .map_err(|_| format!("'{platform}' is not a platform"))?;
    let stack = signed_stack(&candidate.body, platform)
        .map_err(|error| format!("the signed stack does not load: {error}"))?;
    let name = installed.template;
    let settings =
        alien_cli_common::setup_templates::protected_stack_settings(name, &stack, signing)
            .map_err(|error| error.to_string())?;
    let (cloudformation, terraform, helm) = public_registries();
    let options = SetupRenderOptions::builder()
        .signing(signing)
        .cloudformation_registry(&cloudformation)
        .terraform_registry(&terraform)
        .helm_registry(&helm)
        .hosting(record.hosting.clone())
        .pins(pins())
        .renderer(record.renderer.clone())
        .build();
    let template = render_setup_template(name, &stack, &settings, &options)
        .await
        .map_err(|error| {
            let missing = missing_emitters(&error);
            if missing.is_empty() {
                format!("it could not be rendered: {error}")
            } else {
                format!(
                    "the public alien-deploy has no {name} emitter for resource type{} {}, so this template can't be rendered here",
                    if missing.len() == 1 { "" } else { "s" },
                    missing.join(", ")
                )
            }
        })?;
    Ok(match (&installed.content, template.files) {
        (Content::Yaml(text), None) => (text.as_bytes() != template.bytes.as_slice())
            .then(|| "the template differs".to_string()),
        (Content::Files(files), Some(expected)) => first_file_difference(files, &expected),
        _ => Some(
            "the install and the renderer disagree about what kind of template this is".to_string(),
        ),
    })
}

/// The first path (in order) where two file maps differ.
fn first_file_difference(
    installed: &BTreeMap<String, String>,
    expected: &BTreeMap<String, String>,
) -> Option<String> {
    for (path, contents) in installed {
        match expected.get(path) {
            None => return Some(format!("{path} is not part of the rendered template")),
            Some(other) if other != contents => return Some(format!("{path} differs")),
            Some(_) => {}
        }
    }
    expected
        .keys()
        .find(|path| !installed.contains_key(*path))
        .map(|path| format!("{path} is missing"))
}

/// What the hosting inputs say, one fact per line: a hacked control plane picks them, and the
/// customer has to be able to see who may manage the install.
fn hosting_lines(template: &str, record: &SetupRecord) -> Vec<Line> {
    let hosting: &SetupHostingOptions = &record.hosting;
    let mut lines = Vec::new();
    // Only a CloudFormation template carries these inputs: Terraform modules and Helm charts
    // take who manages them as variables and values, which the customer sets.
    if template == "cloudformation" {
        if let Some(registration) = &hosting.registration {
            lines.push(line(
                "Registration",
                registration_text(registration),
                Mark::Info,
            ));
        }
        let principals = &hosting.management_trust.allowed_managing_role_arns;
        lines.push(line(
            "Managing principals",
            if principals.is_empty() {
                "any role the stack is given (none recorded)".to_string()
            } else {
                principals.join(", ")
            },
            Mark::Info,
        ));
        lines.push(line(
            "External id",
            if hosting.management_trust.require_external_id {
                "required"
            } else {
                "not required"
            },
            Mark::Info,
        ));
    } else if !hosting.supported_aws_regions.is_empty() {
        lines.push(line(
            "Regions",
            hosting.supported_aws_regions.join(", "),
            Mark::Info,
        ));
    }
    lines
}

fn registration_text(registration: &alien_cloudformation::RegistrationMode) -> String {
    use alien_cloudformation::RegistrationMode::*;
    match registration {
        CustomResource { .. } | RegionalCustomResource { .. } => "custom resource",
        OutputsFallback => "stack outputs",
        Both { .. } | RegionalBoth { .. } => "custom resource and stack outputs",
    }
    .to_string()
}
