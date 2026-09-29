//! The facts an AWS sandbox's setup renders its scaffolding from. Setup applies them and the
//! runtime never re-reads them, so a change to any of them needs setup to run again.

use alien_error::AlienError;
use serde_json::Value;

use crate::remote_bindings::{
    remote_binding_for_entry, remote_binding_is_deliverable, RemoteBindingDefinition,
};
use crate::sandbox_build_role::SandboxBuildRole;
use crate::sandbox_egress::sandbox_egress_network;
use crate::{
    ErrorData, ResourceLifecycle, Result, Sandbox, SandboxCode, Stack, BUNDLE_REGION_TOKEN,
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

/// The id of the network an AWS sandbox's egress connector attaches to, `None` for `allow`.
/// The error is the refusal message, for the caller to wrap in its own error.
pub fn aws_sandbox_egress_network_id<'a>(
    stack: &'a Stack,
    sandbox: &Sandbox,
) -> std::result::Result<Option<&'a str>, String> {
    let network = match sandbox_egress_network(stack, &sandbox.egress) {
        Ok(Some(network)) => network,
        Ok(None) => return Ok(None),
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

/// The build role setup renders for this sandbox from its bundle.
pub fn aws_sandbox_build_role<'a>(
    sandbox: &'a Sandbox,
    bundle_uri: &'a str,
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

/// Each setup input with the name a refused update reports it by. A new bundle under the same
/// stable prefix, or a new tag in the same private repository, leaves them unchanged.
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
    let policy = aws_sandbox_build_role(sandbox, image, lifecycle, account).policy()?;
    let network = aws_sandbox_egress_network_id(stack, sandbox).map_err(refuse)?;
    // An update that stops publishing keeps the setup-owned Remote Bindings role, so without
    // this the grant would outlive the declaration.
    let grant =
        aws_sandbox_remote_grant(stack, sandbox).map(|definition| definition.permission_set);
    Ok(vec![
        (
            "egress",
            serde_json::to_value(&sandbox.egress).expect("sandbox egress serializes to JSON"),
        ),
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
            vec![(
                "configuration",
                serde_json::to_value(sandbox).expect("a sandbox serializes to JSON"),
            )]
        })
}
