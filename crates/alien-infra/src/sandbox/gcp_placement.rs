//! Which network placement a Google Cloud project's Agent Platform sandboxes have.
//!
//! The first sandbox template a project ever creates fixes how every later sandbox reaches the
//! network: through Google's default network, or through a Private Service Connect interface
//! into the customer's VPC. A template of the other kind is accepted by the create call and then
//! goes to `FAILED` with only `INTERNAL`. No API reads the placement, so this reads what the
//! project shows:
//!
//! * a network attachment with a producer connection means the project is on Private Service
//!   Connect, and the first attachment that ever connected carries the traffic of every sandbox
//!   in the project, whatever a later template names;
//! * an `ACTIVE` template without a network attachment means default placement.
//!
//! A project whose sandboxes were all deleted shows nothing, and the pin stays; the caller then
//! has only the `FAILED` template to explain, which [`PLACEMENT_HINT`] does.

use alien_error::AlienError;
use alien_gcp_clients::agent_platform::SandboxEnvironmentTemplate;
use alien_gcp_clients::compute::NetworkAttachment;

use crate::error::{ErrorData, Result};

use alien_gcp_clients::sandbox_placement::is_connected;
pub(crate) use alien_gcp_clients::sandbox_placement::{Placement, PLACEMENT_HINT};

fn refuse(sandbox_id: &str, message: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message,
        resource_id: Some(sandbox_id.to_string()),
    })
}

/// Refuses a template of `wanted` placement the project cannot host, before it is created. The
/// reading of what the project shows lives in `alien_gcp_clients::sandbox_placement`, which the
/// plan-time check uses too.
pub(crate) fn check_placement(
    sandbox_id: &str,
    wanted: Placement,
    own_attachment: Option<&str>,
    templates: &[SandboxEnvironmentTemplate],
    attachments: &[NetworkAttachment],
) -> Result<()> {
    alien_gcp_clients::sandbox_placement::check_placement(
        wanted,
        own_attachment,
        templates,
        attachments,
    )
    .map_err(|message| refuse(sandbox_id, message))
}

/// What the own attachment shows after its template went `ACTIVE`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Connection {
    /// The tenant connected; these are the interface addresses it drew.
    Connected(Vec<String>),
    /// Not connected yet. Connections have taken 30 to 70 seconds after the template is created.
    NotYet,
    /// The attachment does not exist, so setup did not create it.
    Missing,
}

/// The state of `own_attachment` among the project's `attachments`.
pub(crate) fn connection(own_attachment: &str, attachments: &[NetworkAttachment]) -> Connection {
    match attachments
        .iter()
        .find(|attachment| attachment.name.as_deref() == Some(own_attachment))
    {
        None => Connection::Missing,
        Some(attachment) if !is_connected(attachment) => Connection::NotYet,
        Some(attachment) => Connection::Connected(
            attachment
                .connection_endpoints
                .iter()
                .filter_map(|endpoint| endpoint.ip_address.clone())
                .collect(),
        ),
    }
}

/// Said when a template is `ACTIVE` and its attachment still has no connection after the wait.
/// Measured: a template that failed on the attachment's permission check leaves that attachment
/// unable to connect for good, and later templates on it go `ACTIVE` without ever connecting.
pub(crate) fn never_connected(sandbox_id: &str, attachment: &str) -> AlienError<ErrorData> {
    refuse(
        sandbox_id,
        format!(
            "the sandbox template is ACTIVE but network attachment '{attachment}' never got a \
             connection from Agent Platform, so sandbox traffic would not enter the VPC. This \
             happens for good when a template named the attachment before the Agent Platform \
             service agent could read it (compute.networkAttachments.get). Re-run setup with a \
             new attachment name; a connected attachment cannot be deleted, an unconnected one \
             can"
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_gcp_clients::compute::NetworkAttachmentConnectedEndpoint;

    fn attachment(name: &str, endpoints: usize) -> NetworkAttachment {
        NetworkAttachment {
            name: Some(name.into()),
            connection_endpoints: (0..endpoints)
                .map(|i| NetworkAttachmentConnectedEndpoint {
                    status: Some("ACCEPTED".into()),
                    ip_address: Some(format!("10.30.0.{}", i + 2)),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn a_refusal_names_the_sandbox() {
        let error = check_placement(
            "box",
            Placement::Default,
            None,
            &[],
            &[attachment("own", 2)],
        )
        .expect_err("pinned to PSC");
        match error.error {
            Some(ErrorData::ResourceConfigInvalid { resource_id, .. }) => {
                assert_eq!(resource_id.as_deref(), Some("box"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_attachment_that_never_connected_is_told_apart_from_one_still_connecting() {
        assert_eq!(connection("own", &[]), Connection::Missing);
        assert_eq!(
            connection("own", &[attachment("own", 0)]),
            Connection::NotYet
        );
        assert_eq!(
            connection("own", &[attachment("own", 2)]),
            Connection::Connected(vec!["10.30.0.2".into(), "10.30.0.3".into()])
        );
        let message = never_connected("s", "own").message;
        assert!(message.contains("never got a connection"), "{message}");
        assert!(message.contains("new attachment name"), "{message}");
    }
}
