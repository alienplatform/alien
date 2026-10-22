//! Importer for Azure Network (VNet + subnets + NAT).

use alien_core::{
    Result, StackResourceState,
    import::{ImportContext, data::AzureNetworkImportData},
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::network::{AzureNetworkController, AzureNetworkState};

/// Azure VNet importer.
#[derive(Debug, Default)]
pub struct AzureNetworkImporter;

impl ResourceImporter for AzureNetworkImporter {
    type ImportData = AzureNetworkImportData;

    fn import(
        &self,
        data: AzureNetworkImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let public_subnet_name = data
            .subnet_ids
            .get(0)
            .and_then(|id| subnet_name_from_id(id))
            .map(str::to_string);
        let private_subnet_name = data
            .subnet_ids
            .get(1)
            .and_then(|id| subnet_name_from_id(id))
            .map(str::to_string);
        let application_gateway_subnet_name = data.application_gateway_subnet_name.or_else(|| {
            data.application_gateway_subnet_id
                .as_deref()
                .and_then(subnet_name_from_id)
                .map(str::to_string)
        });
        let _ = (data.subscription_id, data.network_security_group_id);
        let controller = AzureNetworkController {
            state: AzureNetworkState::Ready,
            desired_settings: None,
            sandbox_network: data.sandbox_network,
            vnet_name: data.vnet_name,
            vnet_resource_id: data.vnet_id,
            public_subnet_name,
            private_subnet_name,
            application_gateway_subnet_name,
            // Create-mode networks emit a dedicated PE subnet and carry its name here. A BYO-VNet
            // that omits it reports None; the Postgres controller then fails fast rather than mis-provision.
            private_endpoint_subnet_name: data.private_endpoint_subnet_name,
            container_apps_subnet_name: data.container_apps_subnet_name,
            nat_gateway_name: None,
            nat_gateway_id: data.nat_gateway_id,
            egress_public_ips: data.egress_public_ips,
            public_ip_name: None,
            public_ip_id: None,
            nsg_name: None,
            nsg_id: None,
            resource_group: Some(data.resource_group),
            location: None,
            cidr_block: None,
            is_byo_vnet: data.is_byo_vnet,
            last_byo_vnet_verification_error: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}

fn subnet_name_from_id(id: &str) -> Option<&str> {
    id.rsplit('/').next().filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        Network, NetworkOutputs, Platform, Resource, ResourceEntry, ResourceLifecycle,
        StackSettings,
    };

    #[test]
    fn setup_nat_addresses_survive_import_and_old_imports_remain_readable() {
        let entry = ResourceEntry {
            config: Resource::new(
                Network::new("network".to_string())
                    .settings(alien_core::NetworkSettings::UseDefault)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
            enabled_when: None,
        };
        let settings = StackSettings::default();
        let ctx = ImportContext {
            resource_id: "network",
            platform: Platform::Azure,
            region: "eastus",
            stack_settings: &settings,
            management_config: None,
            resource: &entry,
        };
        let mut json = serde_json::json!({
            "subscriptionId":"sub", "resourceGroup":"group", "vnetId":"/vnet/network", "vnetName":"network",
            "subnetIds":["/vnet/network/subnets/public", "/vnet/network/subnets/private"],
            "natGatewayId":"/nat/gateway", "isByoVnet":false,
            "egressPublicIps":["203.0.113.8"],
            "sandboxNetwork": {
                "sandboxSubnetIds": ["/vnet/network/subnets/sandbox"],
                "gatewaySubnetIds": ["/vnet/network/subnets/gateway"],
                "sandboxCidrs": ["10.0.16.0/23"],
                "sandboxCidrsByResource": {"box": ["10.0.16.0/23"]},
                "gatewayCidrs": ["10.0.6.0/24"],
                "workloadCidrs": ["10.0.0.0/24", "10.0.1.0/24"],
                "vpcCidrs": ["10.0.0.0/16"]
            },
        });
        for version in 0..3 {
            let expected = if version < 2 {
                vec!["203.0.113.8".to_string()]
            } else {
                Vec::new()
            };
            let expected_network = json.get("sandboxNetwork").cloned();
            let data = serde_json::from_value(json.clone()).expect("typed import");
            let state = AzureNetworkImporter.import(data, &ctx).expect("import");
            let outputs = state
                .outputs
                .as_ref()
                .unwrap()
                .downcast_ref::<NetworkOutputs>()
                .unwrap();
            assert_eq!(outputs.egress_public_ips, expected);
            assert!(outputs.has_nat_gateway);
            let restored: AzureNetworkController =
                serde_json::from_value(state.internal_state.unwrap()).expect("controller state");
            assert_eq!(restored.egress_public_ips, expected);
            assert_eq!(
                restored
                    .sandbox_network
                    .map(|network| serde_json::to_value(network).unwrap()),
                expected_network,
            );
            if version == 0 {
                json["sandboxNetwork"]
                    .as_object_mut()
                    .unwrap()
                    .remove("sandboxCidrsByResource");
            } else {
                json.as_object_mut().unwrap().remove("sandboxNetwork");
                json.as_object_mut().unwrap().remove("egressPublicIps");
            }
        }
    }
}
