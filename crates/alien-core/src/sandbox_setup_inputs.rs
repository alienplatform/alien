//! The facts an AWS sandbox's setup renders its scaffolding from. Setup applies them and the
//! runtime never re-reads them, so a change to any of them needs setup to run again.

use alien_error::AlienError;
use serde_json::Value;

use crate::remote_bindings::{
    remote_binding_for_entry, remote_binding_is_deliverable, RemoteBindingDefinition,
};
use crate::sandbox_build_role::{SandboxBuildRole, SANDBOX_BUNDLE_STORAGE_ID};
use crate::sandbox_egress::{sandbox_egress_network, sandbox_needs_connector};
use crate::{
    parse_ecr_image_repository, ErrorData, ResourceEntry, ResourceLifecycle, Result, Sandbox,
    SandboxCode, Stack, Storage, BUNDLE_REGION_TOKEN,
};

/// The account a sandbox's scaffolding is rendered into.
#[derive(Debug, Clone, Copy)]
pub struct SetupAccount<'a> {
    pub partition: &'a str,
    pub account_id: &'a str,
    pub region: &'a str,
}

/// For comparing two stacks without an account. The region stays the token so a `{region}` host
/// and a literal region never render the same ARN.
pub const SETUP_INPUTS_COMPARISON_ACCOUNT: SetupAccount<'static> = SetupAccount {
    partition: "aws",
    account_id: "000000000000",
    region: BUNDLE_REGION_TOKEN,
};

/// The id of the setup-owned network every AWS sandbox connector attaches to.
/// The error is the refusal message, for the caller to wrap in its own error.
pub fn aws_sandbox_egress_network_id<'a>(
    stack: &'a Stack,
    sandbox: &Sandbox,
) -> std::result::Result<Option<&'a str>, String> {
    if !sandbox_needs_connector(stack, sandbox) {
        return Ok(None);
    }
    let network = match sandbox_egress_network(stack) {
        Ok(network) => network,
        Err(refusal) => return Err(refusal.to_string()),
    };
    if network.entry.lifecycle != ResourceLifecycle::Frozen {
        return Err(
            "an AWS sandbox routes session traffic through a VPC egress connector; its network \
             must be created by setup, and this one is created at runtime"
                .to_string(),
        );
    }
    Ok(Some(network.id))
}

/// Whether the controller renders this sandbox's bundle into the deployment's bundle Storage,
/// [`SANDBOX_BUNDLE_STORAGE_ID`]: the sandbox depends on it. Setup adds the Storage for a Live
/// sandbox released as a container image; a deployment set up before it existed builds from the
/// bundle a publisher wrote into the store its setup granted.
pub fn aws_sandbox_renders_bundle(stack: &Stack, sandbox_id: &str) -> bool {
    stack
        .resources
        .get(sandbox_id)
        .is_some_and(aws_sandbox_entry_renders_bundle)
}

/// [`aws_sandbox_renders_bundle`] for the sandbox's own entry.
pub fn aws_sandbox_entry_renders_bundle(entry: &ResourceEntry) -> bool {
    entry.dependencies.iter().any(|dependency| {
        dependency.resource_type == Storage::RESOURCE_TYPE
            && dependency.id == SANDBOX_BUNDLE_STORAGE_ID
    })
}

/// The build role setup renders for this sandbox. With `renders_bundle` it only pulls the image
/// the rendered bundle names; otherwise it reads `bundle_uri`.
pub fn aws_sandbox_build_role<'a>(
    sandbox: &'a Sandbox,
    bundle_uri: &'a str,
    renders_bundle: bool,
    lifecycle: ResourceLifecycle,
    account: SetupAccount<'a>,
) -> SandboxBuildRole<'a> {
    SandboxBuildRole::builder()
        .sandbox_id(&sandbox.id)
        .partition(account.partition)
        .account_id(account.account_id)
        .region(account.region)
        .bundle_uri(bundle_uri)
        .runtime_built(lifecycle == ResourceLifecycle::Live)
        .maybe_private_base_image(sandbox.private_base_image.as_deref())
        .renders_bundle(renders_bundle)
        .build()
}

/// The Remote Bindings grant setup renders for this sandbox, `None` when it publishes none.
pub fn aws_sandbox_remote_grant(
    stack: &Stack,
    sandbox: &Sandbox,
) -> Option<&'static RemoteBindingDefinition> {
    stack
        .resources
        .get(&sandbox.id)
        .filter(|entry| remote_binding_is_deliverable(entry))
        .and_then(remote_binding_for_entry)
}

/// Stands in for a Live sandbox's bundle store in its setup inputs; see [`aws_sandbox_setup_inputs`].
const SETUP_GRANTED_STORE: &str = "setup-granted-store";
/// Stands in for a Live sandbox's private base image repository in its setup inputs.
const SETUP_GRANTED_BASE_IMAGE: &str =
    "000000000000.dkr.ecr.us-east-1.amazonaws.com/setup-granted-repository:base";

/// `image` as [`SETUP_GRANTED_BASE_IMAGE`], keeping its repository's namespace (the path before
/// the last `/`). A publisher keeps installed setups building by writing their bundles `FROM` a
/// repository they granted, which covers a renamed repository in the same namespace. A move to
/// another namespace is not covered, so the namespace stays a setup input.
fn setup_granted_base_image(image: &str) -> std::result::Result<String, String> {
    let repository = parse_ecr_image_repository(image)?;
    Ok(match repository.repository.rsplit_once('/') {
        Some((namespace, _)) => SETUP_GRANTED_BASE_IMAGE.replacen(
            "/setup-granted-repository",
            &format!("/{namespace}/setup-granted-repository"),
            1,
        ),
        None => SETUP_GRANTED_BASE_IMAGE.to_string(),
    })
}

/// `image` with its bucket replaced by [`SETUP_GRANTED_STORE`], keeping the key.
fn setup_granted_bundle(image: &str) -> std::result::Result<String, String> {
    image
        .strip_prefix("s3://")
        .and_then(|path| path.split_once('/'))
        .map(|(_, key)| format!("s3://{SETUP_GRANTED_STORE}/{key}"))
        .ok_or_else(|| format!("bundle '{image}' is not an s3://bucket/key URI"))
}

/// Each setup input with the name a refused update reports it by. A new bundle under the same
/// stable prefix, or a new tag in the same private repository, leaves them unchanged. For a Live
/// sandbox the bundle's store and the base image's repository name are not inputs at all: they
/// are fixed by the setup that granted them. The repository's namespace is.
pub fn aws_sandbox_setup_inputs(
    stack: &Stack,
    sandbox: &Sandbox,
    lifecycle: ResourceLifecycle,
    account: SetupAccount<'_>,
) -> Result<Vec<(&'static str, Value)>> {
    let refuse = |reason: String| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("setup inputs for sandbox '{}'", sandbox.id),
            reason,
        })
    };
    let SandboxCode::Image { image } = &sandbox.code else {
        return Err(refuse(
            "an AWS sandbox is built from a prebuilt s3:// bundle, not from source".to_string(),
        ));
    };
    let policy = if aws_sandbox_renders_bundle(stack, &sandbox.id) {
        // The bundle Storage is a setup-owned resource of its own, compared as one, so only the
        // image grant is an input here.
        aws_sandbox_build_role(sandbox, image, true, lifecycle, account).policy()?
    } else if lifecycle == ResourceLifecycle::Live {
        // A deployment set up before the bundle Storage existed builds from the publisher's
        // bundle, so a release that adds one beside a container image keeps that setup's inputs:
        // it goes on building without a setup rerun, and moves to the Storage when setup runs
        // again.
        //
        // A Live sandbox's store and base image repository are fixed by its setup: the runtime
        // reads each release's bundle key from the store this setup granted, and the publisher
        // puts a bundle there whose base image this role can pull. Only the shape of the grant
        // (stable prefix, whether a private base exists, its repository namespace) is a setup
        // input.
        let image = sandbox.published_bundle.as_deref().unwrap_or(image);
        let mut granted = sandbox.clone();
        granted.private_base_image = sandbox
            .private_base_image
            .as_deref()
            .map(setup_granted_base_image)
            .transpose()
            .map_err(refuse)?;
        let bundle = setup_granted_bundle(image).map_err(refuse)?;
        aws_sandbox_build_role(&granted, &bundle, false, lifecycle, account).policy()?
    } else {
        aws_sandbox_build_role(sandbox, image, false, lifecycle, account).policy()?
    };
    let network = aws_sandbox_egress_network_id(stack, sandbox).map_err(refuse)?;
    // An update that stops publishing keeps the setup-owned Remote Bindings role, so without
    // this the grant would outlive the declaration.
    let grant =
        aws_sandbox_remote_grant(stack, sandbox).map(|definition| definition.permission_set);
    Ok(vec![
        ("egress", sandbox.setup_digest_value()),
        ("egress network", serde_json::json!(network)),
        (
            "build role policy",
            serde_json::to_value(policy).expect("a build role policy serializes to JSON"),
        ),
        ("remote grant", serde_json::json!(grant)),
    ])
}

/// [`aws_sandbox_setup_inputs`] for comparing stacks, which cannot fail: a sandbox whose inputs do
/// not resolve counts as its whole configuration, so any change to it needs setup.
pub fn comparable_aws_sandbox_setup_inputs(
    stack: &Stack,
    sandbox: &Sandbox,
    lifecycle: ResourceLifecycle,
) -> Vec<(&'static str, Value)> {
    aws_sandbox_setup_inputs(stack, sandbox, lifecycle, SETUP_INPUTS_COMPARISON_ACCOUNT)
        .unwrap_or_else(|_| {
            vec![("configuration", {
                let mut value =
                    serde_json::to_value(sandbox).expect("a sandbox serializes to JSON");
                value["egress"] = sandbox.setup_digest_value();
                value
                    .as_object_mut()
                    .expect("sandbox config is an object")
                    .remove("defaultPolicy");
                value
            })]
        })
}
