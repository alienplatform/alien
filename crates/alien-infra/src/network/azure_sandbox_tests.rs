use super::*;
use alien_azure_clients::azure::network::MockNetworkApi;
use serde_json::{Value, json};

const PARENT: &str = "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.Network";
fn arm(kind: &str, name: &str) -> String {
    format!("{PARENT}/{kind}/{name}")
}
fn subnet_id(name: &str) -> String {
    format!("{}/subnets/{name}", arm("virtualNetworks", "vnet"))
}

pub(crate) fn fixture() -> (AzureNetworkController, Value, Value, Value, Value) {
    let network = AzureNetworkController {
        resource_group: Some("rg".into()),
        vnet_name: Some("vnet".into()),
        vnet_resource_id: Some(arm("virtualNetworks", "vnet")),
        private_subnet_name: Some("private".into()),
        nat_gateway_id: Some(arm("natGateways", "nat")),
        egress_public_ips: vec!["203.0.113.10".into()],
        sandbox_network: Some(AzureSandboxNetwork {
            sandbox_subnet_ids: vec![subnet_id("sandbox")],
            gateway_subnet_ids: vec![subnet_id("gateway")],
            sandbox_cidrs: vec!["10.0.8.0/23".into()],
            sandbox_cidrs_by_resource: BTreeMap::from([(
                "sandbox".into(),
                vec!["10.0.8.0/23".into()],
            )]),
            gateway_cidrs: vec!["10.0.6.0/24".into()],
            workload_cidrs: vec!["10.0.1.0/24".into()],
            vpc_cidrs: vec!["10.0.0.0/16".into()],
        }),
        ..Default::default()
    };
    let vnet = json!({"id": arm("virtualNetworks", "vnet"), "location":"eastus", "properties":{
    "addressSpace":{"addressPrefixes":["10.0.0.0/16", "198.18.0.0/16"]},
    "subnets":[
        {"id":subnet_id("sandbox"),"properties":{
            "addressPrefix":"10.0.8.0/23", "natGateway":{"id":arm("natGateways","nat")},
            "networkSecurityGroup":{"id":arm("networkSecurityGroups","sandbox-nsg")},
            "delegations":[{"name":"sandbox", "properties":{"serviceName":"Microsoft.App/environments"}}]}},
        {"id":subnet_id("gateway"),"properties":{"addressPrefix":"10.0.6.0/24"}},
        {"id":subnet_id("private"),"properties":{"addressPrefix":"10.0.1.0/24", "natGateway":{"id":arm("natGateways","nat")}}}
    ]}});
    let mut rules = vec![json!({"name":"gateway","properties":{
        "access":"Allow","direction":"Outbound","priority":100,"protocol":"Tcp",
        "sourceAddressPrefix":"*","sourcePortRange":"*","destinationAddressPrefix":"10.0.6.0/24",
        "destinationPortRanges":["3128","3129"]}})];
    for (destination, priority) in [
        ("AzurePlatformIMDS", 101),
        ("AzurePlatformDNS", 140),
        ("*", 300),
    ] {
        rules.push(json!({"name":format!("deny-{priority}"),"properties":{
            "access":"Deny","direction":"Outbound","priority":priority,"protocol":"*",
            "sourceAddressPrefix":"*","sourcePortRange":"*","destinationAddressPrefix":destination,"destinationPortRange":"*"}}));
    }
    let nsg = json!({"id":arm("networkSecurityGroups","sandbox-nsg"),"location":"eastus","properties":{"securityRules":rules}});
    let nat = json!({"id":arm("natGateways","nat"),"location":"eastus","properties":{"publicIpAddresses":[{"id":arm("publicIPAddresses","public")} ]}});
    let public = json!({"id":arm("publicIPAddresses","public"),"location":"eastus","properties":{"ipAddress":"203.0.113.10"}});
    (network, vnet, nsg, nat, public)
}

async fn check(
    network: &AzureNetworkController,
    vnet: Value,
    nsg: Value,
    nat: Value,
    public: Value,
) -> Result<AzureSandboxNetwork> {
    let client = client(vnet, nsg, nat, public);
    // No write expectations: any attempted repair fails the test.
    validate_azure_sandbox_network(&client, "network", network).await
}

pub(crate) fn client(vnet: Value, nsg: Value, nat: Value, public: Value) -> MockNetworkApi {
    let mut client = MockNetworkApi::new();
    client
        .expect_get_virtual_network()
        .returning(move |group, name| {
            assert_eq!((group, name), ("rg", "vnet"));
            Ok(serde_json::from_value(vnet.clone()).unwrap())
        });
    client
        .expect_get_network_security_group()
        .returning(move |group, name| {
            assert_eq!((group, name), ("rg", "sandbox-nsg"));
            Ok(serde_json::from_value(nsg.clone()).unwrap())
        });
    client
        .expect_get_nat_gateway()
        .returning(move |group, name| {
            assert_eq!((group, name), ("rg", "nat"));
            Ok(serde_json::from_value(nat.clone()).unwrap())
        });
    client
        .expect_get_public_ip_address()
        .returning(move |group, name| {
            assert_eq!((group, name), ("rg", "public"));
            Ok(serde_json::from_value(public.clone()).unwrap())
        });
    client
}

#[tokio::test]
async fn provider_ranges_extend_registration_and_frontend_needs_no_nat() {
    let (network, vnet, nsg, nat, public) = fixture();
    let facts = check(&network, vnet, nsg, nat, public).await.unwrap();
    assert_eq!(facts.vpc_cidrs, ["10.0.0.0/16", "198.18.0.0/16"]);
    assert_eq!(facts.sandbox_cidrs_by_resource["sandbox"], ["10.0.8.0/23"]);
}

#[tokio::test]
async fn refuse_nsg_bypasses_and_missing_platform_denies() {
    for mutation in 0..6 {
        let (network, vnet, mut nsg, nat, public) = fixture();
        let rules = nsg["properties"]["securityRules"].as_array_mut().unwrap();
        match mutation {
            0 => rules[0]["properties"]["destinationAddressPrefix"] = json!("*"),
            1 => rules[0]["properties"]["destinationPortRanges"] = json!(["3128", "3129", "443"]),
            2 => rules[0]["properties"]["protocol"] = json!("*"),
            3 => {
                rules.remove(1);
            }
            4 => {
                rules.remove(2);
            }
            _ => {
                rules.remove(3);
            }
        }
        let error = check(&network, vnet, nsg, nat, public).await.unwrap_err();
        assert!(
            error.to_string().contains("NSG"),
            "mutation {mutation}: {error}"
        );
    }
}

#[tokio::test]
async fn refuse_changed_nat_route_delegation_and_peering() {
    for mutation in 0..6 {
        let (network, mut vnet, nsg, mut nat, mut public) = fixture();
        match mutation {
            0 => {
                vnet["properties"]["subnets"][0]["properties"]["natGateway"]["id"] =
                    json!(arm("natGateways", "other"))
            }
            1 => {
                vnet["properties"]["subnets"][2]["properties"]["natGateway"]["id"] =
                    json!(arm("natGateways", "other"))
            }
            2 => {
                vnet["properties"]["subnets"][0]["properties"]["routeTable"] =
                    json!({"id":arm("routeTables","route")})
            }
            3 => vnet["properties"]["subnets"][0]["properties"]["delegations"] = json!([]),
            4 => {
                nat["properties"]["publicIpPrefixes"] =
                    json!([{"id":arm("publicIPPrefixes","prefix")}])
            }
            _ => public["properties"]["ipAddress"] = json!("203.0.113.11"),
        }
        assert!(
            check(&network, vnet, nsg, nat, public).await.is_err(),
            "mutation {mutation}"
        );
    }
    let (network, mut vnet, nsg, nat, public) = fixture();
    vnet["properties"]["virtualNetworkPeerings"] = json!([{"name":"peer"}]);
    assert!(
        check(&network, vnet, nsg, nat, public)
            .await
            .unwrap_err()
            .to_string()
            .contains("peering")
    );
}

#[tokio::test]
async fn refuse_overlapping_or_forged_source_registration() {
    for mutation in 0..4 {
        let (mut network, vnet, nsg, nat, public) = fixture();
        let declared = network.sandbox_network.as_mut().unwrap();
        match mutation {
            0 => {
                declared
                    .sandbox_cidrs_by_resource
                    .insert("another".into(), vec!["10.0.8.0/23".into()]);
            }
            1 => {
                declared
                    .sandbox_cidrs_by_resource
                    .insert("sandbox".into(), vec!["10.0.1.0/24".into()]);
            }
            2 => declared.workload_cidrs = vec!["10.0.8.0/23".into()],
            _ => declared.gateway_subnet_ids = declared.sandbox_subnet_ids.clone(),
        }
        assert!(
            check(&network, vnet, nsg, nat, public).await.is_err(),
            "mutation {mutation}"
        );
    }
}
