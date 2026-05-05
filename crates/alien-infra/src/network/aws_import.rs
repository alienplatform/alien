//! Importer for AWS Network (VPC + subnets + gateways + security group).

use alien_core::{
    import::{data::AwsNetworkImportData, ImportContext},
    Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::make_imported_state;
use crate::network::{AwsNetworkController, AwsNetworkState};

/// AWS VPC importer.
///
/// Handles both **create** (VPC owned by this stack: VPC ID, gateways,
/// subnets, route tables present) and **BYO-VPC** modes (`is_byo_vpc =
/// true`, gateway/route fields empty). The controller's BYO branch is
/// already idempotent so the heartbeat path will tolerate it.
#[derive(Debug, Default)]
pub struct AwsNetworkImporter;

impl ResourceImporter for AwsNetworkImporter {
    type ImportData = AwsNetworkImportData;

    fn import(
        &self,
        data: AwsNetworkImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let controller = AwsNetworkController {
            state: AwsNetworkState::Ready,
            vpc_id: data.vpc_id,
            cidr_block: data.cidr_block,
            internet_gateway_id: data.internet_gateway_id,
            nat_gateway_id: data.nat_gateway_id,
            eip_allocation_id: data.eip_allocation_id,
            public_subnet_ids: data.public_subnet_ids,
            private_subnet_ids: data.private_subnet_ids,
            public_route_table_id: data.public_route_table_id,
            private_route_table_id: data.private_route_table_id,
            // BYO and create-mode imports never carry transient association
            // IDs — the controller's BYO branch already tolerates `vec![]`.
            route_table_association_ids: Vec::new(),
            security_group_id: data.security_group_id,
            availability_zones: data.availability_zones,
            is_byo_vpc: data.is_byo_vpc,
            _internal_stay_count: None,
        };
        make_imported_state(controller, ctx)
    }
}
