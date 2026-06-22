//! Importer for Azure Network (VNet + subnets + NAT).

use alien_core::{
    import::{data::AzureNetworkImportData, ImportContext},
    Result, StackResourceState,
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
            vnet_name: data.vnet_name,
            vnet_resource_id: data.vnet_id,
            public_subnet_name,
            private_subnet_name,
            application_gateway_subnet_name,
            nat_gateway_name: None,
            nat_gateway_id: data.nat_gateway_id,
            public_ip_name: None,
            public_ip_id: None,
            nsg_name: None,
            nsg_id: None,
            resource_group: Some(data.resource_group),
            location: None,
            cidr_block: None,
            is_byo_vnet: data.is_byo_vnet,
            last_byo_vnet_verification_error_code: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}

fn subnet_name_from_id(id: &str) -> Option<&str> {
    id.rsplit('/').next().filter(|name| !name.is_empty())
}
