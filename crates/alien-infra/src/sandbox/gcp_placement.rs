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

/// How a project's sandboxes reach the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Placement {
    /// Google's default network. Egress `none` and `allow` use it.
    Default,
    /// A Private Service Connect interface into the customer's VPC. Egress `internet` uses it.
    PrivateServiceConnect,
}

/// Said when a template ends `FAILED` with no reason, because a wrong placement fails that way.
pub(crate) const PLACEMENT_HINT: &str = "It also fails this way when the project's sandboxes are \
     fixed to the other kind of networking (Google's default network, or Private Service \
     Connect for egress `internet`): the project's first sandbox decides that for good, and \
     deleting every sandbox does not undo it. Use a project that has never hosted a sandbox";

/// The placement a template asks for. Both settings are needed for Private Service Connect, but
/// either one alone already leaves the default.
pub(crate) fn template_placement(template: &SandboxEnvironmentTemplate) -> Placement {
    let attached = template
        .egress_control_config
        .as_ref()
        .is_some_and(|egress| egress.network_attachment.is_some());
    let private_ingress = template
        .ingress_control_config
        .as_ref()
        .is_some_and(|ingress| ingress.enable_private_service_connect == Some(true));
    if attached || private_ingress {
        Placement::PrivateServiceConnect
    } else {
        Placement::Default
    }
}

fn last_segment(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn is_connected(attachment: &NetworkAttachment) -> bool {
    !attachment.connection_endpoints.is_empty()
}

fn attachment_name(attachment: &NetworkAttachment) -> &str {
    attachment.name.as_deref().unwrap_or("<unnamed>")
}

fn template_name(template: &SandboxEnvironmentTemplate) -> &str {
    template
        .name
        .as_deref()
        .map(last_segment)
        .unwrap_or("<unnamed>")
}

fn refuse(sandbox_id: &str, message: String) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message,
        resource_id: Some(sandbox_id.to_string()),
    })
}

/// Refuses a template of `wanted` placement the project cannot host, before it is created.
///
/// `own_attachment` is the network attachment setup made for this deployment (its name, the last
/// path segment), the one a Private Service Connect template names. `templates` are the project's
/// templates in the sandbox region and `attachments` its network attachments there.
pub(crate) fn check_placement(
    sandbox_id: &str,
    wanted: Placement,
    own_attachment: Option<&str>,
    templates: &[SandboxEnvironmentTemplate],
    attachments: &[NetworkAttachment],
) -> Result<()> {
    // A FAILED template is the wrong-placement symptom itself, so it is not evidence of one.
    let live = |template: &&SandboxEnvironmentTemplate| template.state.as_deref() == Some("ACTIVE");
    let default_template = templates
        .iter()
        .filter(live)
        .find(|template| template_placement(template) == Placement::Default);
    let private_template = templates
        .iter()
        .filter(live)
        .find(|template| template_placement(template) == Placement::PrivateServiceConnect);
    let connected: Vec<&NetworkAttachment> =
        attachments.iter().filter(|a| is_connected(a)).collect();

    match wanted {
        Placement::PrivateServiceConnect => {
            if let Some(template) = default_template {
                return Err(refuse(
                    sandbox_id,
                    format!(
                        "egress `internet` on Google Cloud needs a project whose sandboxes use \
                         Private Service Connect, and this project already hosts a sandbox \
                         template with default networking ('{}'). A project's first sandbox \
                         fixes this for good and deleting it does not undo it. Use a project \
                         that has never hosted a sandbox",
                        template_name(template)
                    ),
                ));
            }
            // Every sandbox of a project leaves through the first attachment that connected.
            if let Some(other) = connected
                .iter()
                .find(|attachment| Some(attachment_name(attachment)) != own_attachment)
            {
                return Err(refuse(
                    sandbox_id,
                    format!(
                        "this project already has a connected sandbox network attachment \
                         ('{}'). All sandboxes in a project send their traffic through the \
                         first attachment that ever connected, so the egress gateway of this \
                         deployment would never see it. Google Cloud supports one sandbox \
                         egress attachment per project: use a project without one, or make the \
                         existing one this deployment's",
                        attachment_name(other)
                    ),
                ));
            }
            Ok(())
        }
        Placement::Default => {
            let evidence = connected
                .first()
                .map(|attachment| {
                    format!(
                        "network attachment '{}' is connected",
                        attachment_name(attachment)
                    )
                })
                .or_else(|| {
                    private_template.map(|template| {
                        format!(
                            "template '{}' uses Private Service Connect",
                            template_name(template)
                        )
                    })
                });
            match evidence {
                Some(evidence) => Err(refuse(
                    sandbox_id,
                    format!(
                        "this project's sandboxes use Private Service Connect ({evidence}), and \
                         a sandbox with default networking would fail with INTERNAL. A \
                         project's first sandbox fixes this for good. Declare egress \
                         `internet`, or use a project that has never hosted a sandbox"
                    ),
                )),
                None => Ok(()),
            }
        }
    }
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
    use alien_gcp_clients::agent_platform::{EgressControlConfig, IngressControlConfig};
    use alien_gcp_clients::compute::NetworkAttachmentConnectedEndpoint;

    fn template(name: &str, state: &str, placement: Placement) -> SandboxEnvironmentTemplate {
        let psc = placement == Placement::PrivateServiceConnect;
        SandboxEnvironmentTemplate {
            name: Some(format!(
                "projects/1/locations/us-central1/reasoningEngines/2/sandboxEnvironmentTemplates/{name}"
            )),
            display_name: None,
            custom_container_environment: None,
            egress_control_config: Some(EgressControlConfig {
                internet_access: Some(false),
                network_attachment: psc.then(|| {
                    "projects/p/regions/us-central1/networkAttachments/own".to_string()
                }),
                extra: Default::default(),
            }),
            ingress_control_config: psc.then(|| IngressControlConfig {
                enable_private_service_connect: Some(true),
                extra: Default::default(),
            }),
            state: Some(state.into()),
            extra: Default::default(),
        }
    }

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
    fn a_fresh_project_hosts_either_placement() {
        for wanted in [Placement::Default, Placement::PrivateServiceConnect] {
            check_placement("s", wanted, Some("own"), &[], &[attachment("own", 0)])
                .unwrap_or_else(|e| panic!("{wanted:?}: {e}"));
        }
    }

    #[test]
    fn internet_is_refused_in_a_project_with_a_default_template() {
        let templates = [template("t1", "ACTIVE", Placement::Default)];
        let error = check_placement(
            "s",
            Placement::PrivateServiceConnect,
            Some("own"),
            &templates,
            &[],
        )
        .expect_err("pinned to default");
        assert!(
            error.message.contains("default networking ('t1')"),
            "{}",
            error.message
        );
        assert!(
            error.message.contains("never hosted a sandbox"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_default_sandbox_is_refused_in_a_project_on_private_service_connect() {
        // A connected attachment is the strongest evidence: it outlives every template.
        let error = check_placement("s", Placement::Default, None, &[], &[attachment("own", 2)])
            .expect_err("pinned to PSC");
        assert!(
            error
                .message
                .contains("network attachment 'own' is connected"),
            "{}",
            error.message
        );
        assert!(error.message.contains("INTERNAL"), "{}", error.message);
        let templates = [template("t", "ACTIVE", Placement::PrivateServiceConnect)];
        let error = check_placement("s", Placement::Default, None, &templates, &[])
            .expect_err("pinned to PSC by a template");
        assert!(
            error
                .message
                .contains("template 't' uses Private Service Connect"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_failed_template_is_the_symptom_not_the_evidence() {
        // A default template that FAILED in a PSC project must not make the project look default.
        let templates = [template("bad", "FAILED", Placement::Default)];
        check_placement(
            "s",
            Placement::PrivateServiceConnect,
            Some("own"),
            &templates,
            &[],
        )
        .expect("a failed template does not pin the project");
    }

    #[test]
    fn the_projects_own_connected_attachment_is_fine() {
        let templates = [template("t", "ACTIVE", Placement::PrivateServiceConnect)];
        check_placement(
            "s",
            Placement::PrivateServiceConnect,
            Some("own"),
            &templates,
            &[attachment("own", 2), attachment("idle", 0)],
        )
        .expect("an upgrade or replace of the same deployment passes");
    }

    #[test]
    fn another_connected_attachment_would_carry_the_traffic_and_is_refused() {
        let error = check_placement(
            "s",
            Placement::PrivateServiceConnect,
            Some("own"),
            &[],
            &[attachment("other", 2), attachment("own", 2)],
        )
        .expect_err("traffic would use the first attachment");
        assert!(error.message.contains("'other'"), "{}", error.message);
        assert!(
            error
                .message
                .contains("one sandbox egress attachment per project"),
            "{}",
            error.message
        );
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

    #[test]
    fn the_template_wire_shape_decides_placement() {
        // What the API answered for a default template: `ingressControlConfig` is null.
        let default: SandboxEnvironmentTemplate = serde_json::from_value(serde_json::json!({
            "name": "t", "state": "ACTIVE", "egressControlConfig": {}, "ingressControlConfig": null
        }))
        .unwrap();
        assert_eq!(template_placement(&default), Placement::Default);
        let psc: SandboxEnvironmentTemplate = serde_json::from_value(serde_json::json!({
            "name": "t", "state": "ACTIVE",
            "egressControlConfig": {"networkAttachment": "projects/p/regions/r/networkAttachments/a"},
            "ingressControlConfig": {"enablePrivateServiceConnect": true}
        }))
        .unwrap();
        assert_eq!(template_placement(&psc), Placement::PrivateServiceConnect);
    }
}
