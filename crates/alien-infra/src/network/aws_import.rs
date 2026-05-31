//! Importer for AWS Network (VPC + subnets + gateways + security group).

use alien_core::{
    import::{data::AwsNetworkImportData, ImportContext},
    NetworkSettings, ResourceStatus, Result, StackResourceState,
};

use crate::import::ResourceImporter;
use crate::import_helpers::{make_imported_state, make_imported_state_with_status};
use crate::network::{AwsNetworkController, AwsNetworkState};

/// AWS VPC importer.
///
/// The setup artifact is only responsible for handing off concrete IDs it can
/// know. `create` and `byo-vpc-aws` imports include a VPC ID and can become
/// `Ready` immediately. `use-default` CloudFormation imports intentionally
/// carry no VPC/subnet IDs, so the imported controller resumes at `CreateStart`
/// and lets the normal AWS controller discover the account default VPC before
/// compute/container resources consume the network dependency.
#[derive(Debug, Default)]
pub struct AwsNetworkImporter;

impl ResourceImporter for AwsNetworkImporter {
    type ImportData = AwsNetworkImportData;

    fn import(
        &self,
        data: AwsNetworkImportData,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let needs_default_vpc_discovery = matches!(
            ctx.stack_settings.network.as_ref(),
            Some(NetworkSettings::UseDefault)
        ) && data.vpc_id.is_none();
        let is_setup_owned_vpc = data.is_byo_vpc
            || matches!(
                ctx.stack_settings.network.as_ref(),
                Some(NetworkSettings::UseDefault | NetworkSettings::ByoVpcAws { .. })
            );

        let controller = AwsNetworkController {
            state: if needs_default_vpc_discovery {
                AwsNetworkState::CreateStart
            } else {
                AwsNetworkState::Ready
            },
            vpc_id: data.vpc_id,
            cidr_block: data.cidr_block,
            internet_gateway_id: data.internet_gateway_id,
            nat_gateway_id: data.nat_gateway_id,
            eip_allocation_id: data.eip_allocation_id,
            public_subnet_ids: data.public_subnet_ids,
            private_subnet_ids: data.private_subnet_ids,
            public_route_table_id: data.public_route_table_id,
            private_route_table_id: data.private_route_table_id,
            // Setup imports never carry transient association IDs. Create-mode
            // imports already include the stable route-table IDs; use-default
            // imports rediscover the provider-owned subnets at runtime.
            route_table_association_ids: Vec::new(),
            security_group_id: data.security_group_id,
            availability_zones: data.availability_zones,
            is_byo_vpc: is_setup_owned_vpc,
            _internal_stay_count: None,
        };
        if needs_default_vpc_discovery {
            make_imported_state_with_status(controller, ctx, ResourceStatus::Provisioning)
        } else {
            make_imported_state(controller, ctx)
        }
    }
}

#[cfg(test)]
mod tests {
    use alien_core::{
        import::{data::AwsNetworkImportData, ImportContext},
        AwsManagementConfig, ManagementConfig, Network, Platform, Resource, ResourceEntry,
        ResourceLifecycle, StackSettings,
    };

    use super::*;

    fn import_context<'a>(
        settings: &'a StackSettings,
        entry: &'a ResourceEntry,
    ) -> ImportContext<'a> {
        static MANAGEMENT: std::sync::LazyLock<ManagementConfig> = std::sync::LazyLock::new(|| {
            ManagementConfig::Aws(AwsManagementConfig {
                managing_role_arn: "arn:aws:iam::111122223333:role/manager".to_string(),
            })
        });

        ImportContext {
            resource_id: "default-network",
            platform: Platform::Aws,
            region: "us-east-2",
            stack_settings: settings,
            management_config: Some(&MANAGEMENT),
            resource: entry,
        }
    }

    fn network_entry() -> ResourceEntry {
        ResourceEntry {
            config: Resource::new(
                Network::new("default-network".to_string())
                    .settings(NetworkSettings::UseDefault)
                    .build(),
            ),
            lifecycle: ResourceLifecycle::Frozen,
            dependencies: Vec::new(),
            remote_access: false,
        }
    }

    fn empty_default_import_data() -> AwsNetworkImportData {
        AwsNetworkImportData {
            vpc_id: None,
            cidr_block: None,
            internet_gateway_id: None,
            nat_gateway_id: None,
            eip_allocation_id: None,
            public_subnet_ids: Vec::new(),
            private_subnet_ids: Vec::new(),
            public_route_table_id: None,
            private_route_table_id: None,
            security_group_id: None,
            availability_zones: Vec::new(),
            is_byo_vpc: true,
        }
    }

    #[test]
    fn use_default_import_defers_to_runtime_default_vpc_discovery() {
        let settings = StackSettings {
            network: Some(NetworkSettings::UseDefault),
            ..StackSettings::default()
        };
        let entry = network_entry();
        let imported = AwsNetworkImporter
            .import(
                empty_default_import_data(),
                &import_context(&settings, &entry),
            )
            .expect("network import should succeed");

        assert_eq!(imported.status, ResourceStatus::Provisioning);
        let internal = imported
            .internal_state
            .expect("imported network should have controller state");
        assert_eq!(internal["state"], "createStart");
        assert_eq!(internal["isByoVpc"], true);
        assert!(imported.outputs.is_none());
    }

    #[test]
    fn imported_network_with_vpc_id_is_ready() {
        let settings = StackSettings {
            network: Some(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec!["subnet-public".to_string()],
                private_subnet_ids: vec!["subnet-private".to_string()],
                security_group_ids: vec!["sg-123".to_string()],
            }),
            ..StackSettings::default()
        };
        let mut data = empty_default_import_data();
        data.vpc_id = Some("vpc-123".to_string());
        data.public_subnet_ids = vec!["subnet-public".to_string()];
        data.private_subnet_ids = vec!["subnet-private".to_string()];
        data.security_group_id = Some("sg-123".to_string());
        let entry = network_entry();
        let imported = AwsNetworkImporter
            .import(data, &import_context(&settings, &entry))
            .expect("network import should succeed");

        assert_eq!(imported.status, ResourceStatus::Running);
        let internal = imported
            .internal_state
            .expect("imported network should have controller state");
        assert_eq!(internal["state"], "ready");
        assert_eq!(internal["isByoVpc"], true);
        assert!(imported.outputs.is_some());
    }

    #[test]
    fn stack_settings_mark_byo_vpc_import_as_setup_owned_even_if_data_is_stale() {
        let settings = StackSettings {
            network: Some(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec!["subnet-public".to_string()],
                private_subnet_ids: vec!["subnet-private".to_string()],
                security_group_ids: vec!["sg-123".to_string()],
            }),
            ..StackSettings::default()
        };
        let mut data = empty_default_import_data();
        data.vpc_id = Some("vpc-123".to_string());
        data.public_subnet_ids = vec!["subnet-public".to_string()];
        data.private_subnet_ids = vec!["subnet-private".to_string()];
        data.security_group_id = Some("sg-123".to_string());
        data.is_byo_vpc = false;
        let entry = network_entry();
        let imported = AwsNetworkImporter
            .import(data, &import_context(&settings, &entry))
            .expect("network import should succeed");

        let internal = imported
            .internal_state
            .expect("imported network should have controller state");
        assert_eq!(internal["isByoVpc"], true);
    }
}
