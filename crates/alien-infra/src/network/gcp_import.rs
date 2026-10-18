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
        // Recover the subnet name from its self-link (`.../subnetworks/<name>`) so the imported
        // Frozen network exposes the same `subnetwork_name` the runtime create path sets. Without
        // it a live worker's `get_vpc_access` short-circuits on the `subnetwork_name` check and the
        // Cloud Run service gets no Direct VPC egress — unable to reach a private Cloud SQL PSC
        // endpoint in this subnet. Mirrors the Azure importer's self-link name parse.
        let subnetwork_name = subnetwork_self_link
            .as_deref()
            .and_then(|link| link.rsplit('/').next())
            .filter(|name| !name.is_empty())
            .map(str::to_string);
        let _ = data.project_id;

        let controller = GcpNetworkController {
            state: GcpNetworkState::Ready,
            desired_settings: None,
            network_name: data.vpc_name,
            network_self_link: data.vpc_self_link,
            subnetwork_name,
            subnetwork_self_link,
            router_name: None,
            cloud_nat_name: data.nat_name,
            egress_public_ips: data.egress_public_ips,
            firewall_name: None,
            // Region is still resolved from stack settings or the cloud
            // self-link; the CIDR comes from the imported subnetwork so
            // dependent resources can create scoped firewall rules.
            region: None,
            cidr_block: data.cidr_block,
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
            platform: Platform::Gcp,
            region: "us-central1",
            stack_settings: &settings,
            management_config: None,
            resource: &entry,
        };
        let mut json = serde_json::json!({
            "projectId":"project", "vpcSelfLink":"/projects/project/global/networks/network",
            "vpcName":"network", "subnetSelfLinks":["/regions/us-central1/subnetworks/workload"],
            "natName":"nat", "isByoVpc":false, "egressPublicIps":["203.0.113.8"],
        });
        for expected in [vec!["203.0.113.8".to_string()], Vec::new()] {
            let data = serde_json::from_value(json.clone()).expect("typed import");
            let state = GcpNetworkImporter.import(data, &ctx).expect("import");
            let outputs = state
                .outputs
                .as_ref()
                .unwrap()
                .downcast_ref::<NetworkOutputs>()
                .unwrap();
            assert_eq!(outputs.egress_public_ips, expected);
            assert!(outputs.has_nat_gateway);
            let restored: GcpNetworkController =
                serde_json::from_value(state.internal_state.unwrap()).expect("controller state");
            assert_eq!(restored.egress_public_ips, expected);
            json.as_object_mut().unwrap().remove("egressPublicIps");
        }
    }
}
