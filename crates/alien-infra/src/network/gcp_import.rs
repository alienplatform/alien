//! Importer for GCP Network (VPC + Cloud NAT + subnetworks).

use alien_core::{
    import::{data::GcpNetworkImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::network::{GcpNetworkController, GcpNetworkState};

/// GCP VPC importer.
///
/// Both create and BYO-VPC modes share the same controller shape; the
/// `is_byo_vpc` flag drives heartbeat-time behavior.
#[derive(Debug, Default)]
pub struct GcpNetworkImporter;

impl ResourceImporter for GcpNetworkImporter {
    type ImportData = GcpNetworkImportData;

    fn import(
        &self,
        data: GcpNetworkImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        // GCP exposes a single subnetwork per region; the importer uses the
        // first entry from the wire payload and stores the rest as
        // metadata-only via the network self-link.
        let subnetwork_self_link = data.subnet_self_links.first().cloned();
        let _ = data.project_id;

        let controller = GcpNetworkController {
            state: GcpNetworkState::Ready,
            desired_settings: None,
            network_name: data.vpc_name,
            network_self_link: data.vpc_self_link,
            subnetwork_name: None,
            subnetwork_self_link,
            router_name: None,
            cloud_nat_name: data.nat_name,
            firewall_name: None,
            // Region and CIDR live in `StackSettings.network` and are picked
            // up by the controller's heartbeat path.
            region: None,
            cidr_block: None,
            is_byo_vpc: data.is_byo_vpc,
            pending_operation_name: None,
            pending_operation_region: None,
            _internal_stay_count: None,
        };
        // router_self_link is informational and reconstructed from project
        // + region + name when needed.
        let _ = data.router_self_link;
        make_imported_state(controller, ctx)
    }
}
