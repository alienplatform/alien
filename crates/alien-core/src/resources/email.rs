use crate::error::{ErrorData, Result};
use crate::resource::{ResourceDefinition, ResourceOutputsDefinition, ResourceRef};
use crate::ResourceType;
use alien_error::AlienError;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Debug;

/// Email infrastructure for sending and receiving mail on customer-owned
/// domains. On AWS this is backed by SES: a shared configuration set, optional
/// inbound/event wiring, and one email identity (Easy DKIM) per seed domain.
///
/// # Infrastructure vs runtime data
///
/// This resource owns the email *infrastructure* and the capability to use it,
/// not the domain lifecycle. Deployment manages the configuration set, the
/// event topology, the inbound receipt topology, and any seed identities
/// listed in `domains`. Email identities created at runtime through the
/// `email/manage-identities` grant are application data: they are not tracked
/// by the deployment, are not removed when the stack is deleted, and their
/// lifecycle — including deletion — belongs to the application.
///
/// The operator (or the application, for runtime-created identities) owns DNS:
/// the per-domain DKIM CNAME records surfaced in [`EmailOutputs`] must be
/// created before SES verifies a domain and allows sending from it.
///
/// # Inbound mail (AWS)
///
/// When `inbound` is set, a SES receipt rule set is provisioned that writes
/// raw incoming mail into the linked Storage bucket. The receipt rule is a
/// catch-all (no recipient filter), so mail for identities verified at runtime
/// lands in the bucket without any infrastructure change.
///
/// Alien activates the provisioned receipt rule set as part of setup. Because
/// SES permits only one active receipt rule set per AWS account and region, an
/// AWS stack may contain only one email resource with inbound delivery, and
/// installing it makes its rule set the account's active rule set. SES email
/// receiving is available only in a subset of AWS regions; deploying `inbound`
/// to an unsupported region fails during setup.
///
/// # Update semantics
///
/// `domains` is append-friendly: adding a domain provisions a new identity,
/// removing a domain deletes its identity (and its DKIM verification state).
/// The list may be empty. `inbound` and `events` may be added, removed, or
/// repointed; removing them tears down the corresponding receipt rule set /
/// event destination wiring.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct Email {
    /// Identifier for the email resource. Must contain only alphanumeric
    /// characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
    #[builder(start_fn)]
    pub id: String,

    /// Seed mail domains provisioned at deploy time (one SES identity each).
    /// Useful for day-0 bootstrap and products with a static domain set.
    /// May be empty (the default): products that create and verify domains
    /// dynamically should manage identities at runtime through the
    /// `email/manage-identities` grant instead of listing them here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub domains: Vec<String>,

    /// Optional inbound-mail configuration: raw incoming mail (for any
    /// identity the account receives mail for — the receipt rule is a
    /// catch-all) is written to the linked Storage bucket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbound: Option<EmailInbound>,

    /// Optional sending-event configuration: send / delivery / bounce /
    /// complaint / delivery-delay / reject events are delivered to the
    /// linked Queue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<EmailEvents>,
}

/// Inbound-mail configuration for an [`Email`] resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmailInbound {
    /// The Storage resource that receives raw incoming mail objects.
    pub storage: ResourceRef,
}

/// Sending-event configuration for an [`Email`] resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmailEvents {
    /// The Queue resource that receives sending events.
    pub queue: ResourceRef,
}

impl Email {
    /// The resource type identifier for Email.
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("email");

    /// Returns the email resource's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// A single DKIM CNAME record the operator must create in DNS.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EmailDkimToken {
    /// CNAME record host name.
    pub name: String,
    /// CNAME record value.
    pub value: String,
}

/// Per-domain DNS records the operator must create.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EmailDomainOutputs {
    /// Easy-DKIM CNAME tokens (three per domain). The domain is verified once
    /// these records exist in its DNS configuration.
    pub dkim_tokens: Vec<EmailDkimToken>,
}

/// Outputs generated by a successfully provisioned Email resource.
///
/// Domain verification status cannot be known at provisioning time — SES
/// verifies a domain asynchronously once its DKIM records exist in DNS — so
/// the outputs carry the records the operator must create rather than a
/// verification result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct EmailOutputs {
    /// DNS records per mail domain.
    pub domains: BTreeMap<String, EmailDomainOutputs>,
    /// The provisioned configuration set name (used when sending).
    pub configuration_set: String,
    /// The inbound receipt rule set name, when `inbound` is configured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_set_name: Option<String>,
}

impl ResourceOutputsDefinition for EmailOutputs {
    fn get_resource_type(&self) -> ResourceType {
        Email::RESOURCE_TYPE.clone()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceOutputsDefinition> {
        Box::new(self.clone())
    }

    fn outputs_eq(&self, other: &dyn ResourceOutputsDefinition) -> bool {
        other.as_any().downcast_ref::<EmailOutputs>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

impl ResourceDefinition for Email {
    fn get_resource_type(&self) -> ResourceType {
        Self::RESOURCE_TYPE
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn get_dependencies(&self) -> Vec<ResourceRef> {
        let mut dependencies = Vec::new();
        if let Some(inbound) = &self.inbound {
            dependencies.push(inbound.storage.clone());
        }
        if let Some(events) = &self.events {
            dependencies.push(events.queue.clone());
        }
        dependencies
    }

    fn validate_update(&self, new_config: &dyn ResourceDefinition) -> Result<()> {
        let new_email = new_config.as_any().downcast_ref::<Email>().ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResourceType {
                resource_id: self.id.clone(),
                expected: Self::RESOURCE_TYPE,
                actual: new_config.get_resource_type(),
            })
        })?;

        if self.id != new_email.id {
            return Err(AlienError::new(ErrorData::InvalidResourceUpdate {
                resource_id: self.id.clone(),
                reason: "the 'id' field is immutable".to_string(),
            }));
        }

        // Seed domains are append-friendly: adding provisions a new identity
        // and removing deletes one (including its DKIM verification state).
        // An empty list is valid — runtime-created identities are managed
        // outside the deployment.

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn box_clone(&self) -> Box<dyn ResourceDefinition> {
        Box::new(self.clone())
    }

    fn resource_eq(&self, other: &dyn ResourceDefinition) -> bool {
        other.as_any().downcast_ref::<Email>() == Some(self)
    }

    fn to_json_value(&self) -> serde_json::Result<serde_json::Value> {
        serde_json::to_value(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::{Queue, Storage};

    fn email_with_links() -> Email {
        let storage = Storage::new("mailbox".to_string()).build();
        let queue = Queue::new("mail-events".to_string()).build();
        Email::new("mailer".to_string())
            .domains(vec!["mail.example.com".to_string()])
            .inbound(EmailInbound {
                storage: ResourceRef::from(&storage),
            })
            .events(EmailEvents {
                queue: ResourceRef::from(&queue),
            })
            .build()
    }

    #[test]
    fn builder_produces_expected_config() {
        let email = email_with_links();
        assert_eq!(email.id, "mailer");
        assert_eq!(email.domains, vec!["mail.example.com"]);
        assert_eq!(
            email.inbound.as_ref().expect("inbound").storage.id,
            "mailbox"
        );
        assert_eq!(
            email.events.as_ref().expect("events").queue.id,
            "mail-events"
        );
    }

    #[test]
    fn resource_type_is_email() {
        assert_eq!(Email::RESOURCE_TYPE.as_ref(), "email");
    }

    #[test]
    fn dependencies_include_inbound_storage_and_events_queue() {
        let email = email_with_links();
        let dependencies = email.get_dependencies();
        assert_eq!(dependencies.len(), 2);
        assert_eq!(dependencies[0].resource_type, Storage::RESOURCE_TYPE);
        assert_eq!(dependencies[0].id, "mailbox");
        assert_eq!(dependencies[1].resource_type, Queue::RESOURCE_TYPE);
        assert_eq!(dependencies[1].id, "mail-events");
    }

    #[test]
    fn dependencies_are_empty_without_links() {
        let email = Email::new("mailer".to_string())
            .domains(vec!["mail.example.com".to_string()])
            .build();
        assert!(email.get_dependencies().is_empty());
    }

    #[test]
    fn validate_update_rejects_id_change() {
        let original = Email::new("mailer".to_string())
            .domains(vec!["mail.example.com".to_string()])
            .build();
        let renamed = Email::new("other".to_string())
            .domains(vec!["mail.example.com".to_string()])
            .build();

        let error = original
            .validate_update(&renamed)
            .expect_err("changing the id must be rejected");
        assert!(error.to_string().contains("'id' field is immutable"));
    }

    #[test]
    fn validate_update_allows_adding_and_removing_domains() {
        let original = Email::new("mailer".to_string())
            .domains(vec![
                "mail.example.com".to_string(),
                "mail.example.org".to_string(),
            ])
            .build();
        let appended = Email::new("mailer".to_string())
            .domains(vec![
                "mail.example.com".to_string(),
                "mail.example.org".to_string(),
                "mail.example.net".to_string(),
            ])
            .build();
        let removed = Email::new("mailer".to_string())
            .domains(vec!["mail.example.com".to_string()])
            .build();

        original
            .validate_update(&appended)
            .expect("adding a domain must be allowed");
        original
            .validate_update(&removed)
            .expect("removing a domain must be allowed");
    }

    #[test]
    fn validate_update_allows_removing_all_seed_domains() {
        let original = Email::new("mailer".to_string())
            .domains(vec!["mail.example.com".to_string()])
            .build();
        let emptied = Email::new("mailer".to_string()).build();

        original
            .validate_update(&emptied)
            .expect("removing all seed domains must be allowed");
    }

    #[test]
    fn builder_defaults_to_no_seed_domains() {
        let email = Email::new("mailer".to_string()).build();
        assert!(email.domains.is_empty());
        assert!(email.inbound.is_none());
        assert!(email.events.is_none());
        assert!(email.get_dependencies().is_empty());
    }

    #[test]
    fn empty_domains_are_omitted_from_serialization_and_roundtrip() {
        let email = Email::new("mailer".to_string()).build();
        let json = serde_json::to_value(&email).expect("email should serialize");
        assert_eq!(json, serde_json::json!({ "id": "mailer" }));

        let roundtrip: Email = serde_json::from_value(json).expect("email should deserialize");
        assert_eq!(email, roundtrip);
    }

    #[test]
    fn validate_update_allows_link_changes() {
        let original = email_with_links();
        let unlinked = Email::new("mailer".to_string())
            .domains(vec!["mail.example.com".to_string()])
            .build();

        original
            .validate_update(&unlinked)
            .expect("removing inbound/events must be allowed");
        unlinked
            .validate_update(&original)
            .expect("adding inbound/events must be allowed");
    }

    #[test]
    fn serializes_with_camel_case_and_roundtrips() {
        let email = email_with_links();
        let json = serde_json::to_value(&email).expect("email should serialize");
        assert_eq!(json["domains"][0], "mail.example.com");
        assert_eq!(json["inbound"]["storage"]["id"], "mailbox");
        assert_eq!(json["inbound"]["storage"]["type"], "storage");
        assert_eq!(json["events"]["queue"]["id"], "mail-events");

        let roundtrip: Email = serde_json::from_value(json).expect("email should deserialize");
        assert_eq!(email, roundtrip);
    }

    #[test]
    fn outputs_roundtrip() {
        let outputs = EmailOutputs {
            domains: BTreeMap::from([(
                "mail.example.com".to_string(),
                EmailDomainOutputs {
                    dkim_tokens: vec![EmailDkimToken {
                        name: "token._domainkey.mail.example.com".to_string(),
                        value: "token.dkim.amazonses.com".to_string(),
                    }],
                },
            )]),
            configuration_set: "stack-mailer".to_string(),
            rule_set_name: Some("stack-mailer".to_string()),
        };
        let json = serde_json::to_string(&outputs).expect("outputs should serialize");
        let deserialized: EmailOutputs =
            serde_json::from_str(&json).expect("outputs should deserialize");
        assert_eq!(outputs, deserialized);
    }
}
