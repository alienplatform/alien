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
        // Azure splits subnets between public + private; the wire payload
        // uses a flat list. The heartbeat path will rebuild the
        // public/private split from the network config.
        let _ = (
            data.subscription_id,
            data.subnet_ids,
            data.network_security_group_id,
        );
        let controller = AzureNetworkController {
            state: AzureNetworkState::Ready,
            desired_settings: None,
            vnet_name: data.vnet_name,
            vnet_resource_id: data.vnet_id,
            public_subnet_name: None,
            private_subnet_name: None,
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
            last_byo_vnet_verification_error: None,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
