use super::*;

const SERVICE_ATTACHMENT: &str =
    "https://www.googleapis.com/compute/v1/projects/p-producer/regions/us-east1/serviceAttachments/sql-sa";
const STACK_SUBNET: &str =
    "https://www.googleapis.com/compute/v1/projects/p-consumer/regions/us-east1/subnetworks/stack-subnet";
const STACK_NETWORK: &str =
    "https://www.googleapis.com/compute/v1/projects/p-consumer/global/networks/stack-vpc";

/// The forwarding rule half of a Private Service Connect consumer endpoint:
/// it targets the producer's service attachment over an internal IP, in the
/// consumer's network/subnet, and is *not* a load balancer.
fn psc_consumer_forwarding_rule() -> ForwardingRule {
    ForwardingRule {
        name: Some("stack-psc-endpoint".into()),
        target: Some(SERVICE_ATTACHMENT.into()),
        ip_address: Some("10.0.0.42".into()),
        network: Some(STACK_NETWORK.into()),
        subnetwork: Some(STACK_SUBNET.into()),
        // PSC consumer endpoints are not load balancers: scheme stays unset.
        load_balancing_scheme: None,
        ..Default::default()
    }
}

/// The address half of a PSC consumer endpoint: a regional INTERNAL IP
/// reserved from the consumer's subnet.
fn psc_consumer_address() -> Address {
    Address {
        name: Some("stack-psc-ip".into()),
        address_type: Some(AddressType::Internal),
        address: Some("10.0.0.42".into()),
        subnetwork: Some(STACK_SUBNET.into()),
        ..Default::default()
    }
}

#[test]
fn psc_forwarding_rule_serializes_for_consumer_endpoint() {
    let json = serde_json::to_value(psc_consumer_forwarding_rule())
        .expect("forwarding rule should serialize");

    assert_eq!(json["name"], "stack-psc-endpoint");
    // The target is the producer service attachment — this is what makes it PSC.
    assert_eq!(json["target"], SERVICE_ATTACHMENT);
    // Internal reachability: a fixed internal IP in the consumer subnet.
    assert_eq!(json["IPAddress"], "10.0.0.42");
    assert_eq!(json["network"], STACK_NETWORK);
    assert_eq!(json["subnetwork"], STACK_SUBNET);
    // A PSC consumer endpoint must NOT carry a load-balancing scheme.
    assert!(
        json.get("loadBalancingScheme").is_none(),
        "PSC consumer endpoint must not set loadBalancingScheme, got {json:?}"
    );
    // No global-only target-proxy ports leak in.
    assert!(json.get("portRange").is_none());
    // GCP rejects IPProtocol on a service-attachment-target (PSC) rule outright, so it
    // must be omitted entirely.
    assert!(
        json.get("IPProtocol").is_none(),
        "PSC consumer endpoint must not set IPProtocol, got {json:?}"
    );
}

#[test]
fn psc_address_serializes_as_regional_internal() {
    let json = serde_json::to_value(psc_consumer_address()).expect("address should serialize");

    assert_eq!(json["name"], "stack-psc-ip");
    // Must be INTERNAL — an external address can't back a PSC endpoint.
    assert_eq!(json["addressType"], "INTERNAL");
    assert_eq!(json["address"], "10.0.0.42");
    // The internal IP is drawn from the consumer subnet.
    assert_eq!(json["subnetwork"], STACK_SUBNET);
    // No external-only fields should appear.
    assert!(json.get("networkTier").is_none());
}

#[test]
fn forwarding_rule_round_trips_through_get_response() {
    // A GET on the rule returns the same identity fields we sent on insert.
    let rule: ForwardingRule =
        serde_json::from_value(serde_json::to_value(psc_consumer_forwarding_rule()).unwrap())
            .expect("forwarding rule should deserialize");

    assert_eq!(rule.name.as_deref(), Some("stack-psc-endpoint"));
    assert_eq!(rule.target.as_deref(), Some(SERVICE_ATTACHMENT));
    assert_eq!(rule.subnetwork.as_deref(), Some(STACK_SUBNET));
    assert!(rule.load_balancing_scheme.is_none());
}
