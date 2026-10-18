//! Importer for AWS Network (VPC + subnets + gateways + security group).

use std::collections::BTreeSet;

use alien_core::{
    import::{data::AwsNetworkImportData, ImportContext},
    ErrorData as CoreErrorData, Network, NetworkSettings, ResourceStatus, Result,
    StackResourceState,
};
use alien_error::{AlienError, Context, IntoAlienError};

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
        // An existing VPC handed off without any subnet IDs is not ready: the
        // discovery step rejects it instead of reporting a network with nothing to
        // place workloads in.
        let needs_subnet_domain_discovery = !needs_default_vpc_discovery
            && data.subnets_by_failure_domain.is_empty()
            && (!data.public_subnet_ids.is_empty()
                || !data.private_subnet_ids.is_empty()
                || data.vpc_id.is_some());
        let is_setup_owned_vpc = data.is_byo_vpc
            || matches!(
                ctx.stack_settings.network.as_ref(),
                Some(NetworkSettings::UseDefault | NetworkSettings::ByoVpcAws { .. })
            );

        let controller = AwsNetworkController {
            state: if needs_default_vpc_discovery {
                AwsNetworkState::CreateStart
            } else if needs_subnet_domain_discovery {
                AwsNetworkState::DiscoveringSubnetFailureDomains
            } else {
                AwsNetworkState::Ready
            },
            vpc_id: data.vpc_id,
            cidr_block: data.cidr_block,
            internet_gateway_id: data.internet_gateway_id,
            nat_gateway_id: data.nat_gateway_id,
            eip_allocation_id: data.eip_allocation_id,
            egress_public_ips: data.egress_public_ips,
            sandbox_network: data.sandbox_network,
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
            subnets_by_failure_domain: data.subnets_by_failure_domain,
            is_byo_vpc: is_setup_owned_vpc,
            vpc_create_token: None,
            subnet_create_attempt: None,
            nat_gateway_create_token: None,
            internet_gateway_create_token: None,
            eip_create_token: None,
            nat_gateway_attempt: 0,
            wait_for_create_lookup_iterations: 0,
            wait_for_repeated_create_iterations: 0,
            extra_vpc_ids: Vec::new(),
            extra_internet_gateway_ids: Vec::new(),
            extra_eip_allocation_ids: Vec::new(),
            wait_for_delete_dependencies_iterations: 0,
            wait_for_retained_delete_iterations: 0,
            _internal_stay_count: None,
        };
        if needs_default_vpc_discovery || needs_subnet_domain_discovery {
            make_imported_state_with_status(controller, ctx, ResourceStatus::Provisioning)
        } else {
            make_imported_state(controller, ctx)
        }
    }

    /// Setup payloads never carry what the controller discovers at runtime: the
    /// subnet-to-availability-zone map, and in use-default mode the VPC itself.
    /// Replacing a Ready network with a fresh import would throw that away and
    /// send the network back through discovery on every setup re-registration
    /// (for example any CloudFormation stack update). Dependents that are already
    /// Running keep stepping meanwhile and would read an empty map.
    ///
    /// So the discovered facts are kept when the existing network is Ready with
    /// a discovered zone map, its network mode is unchanged, and the import names
    /// the same VPC and subnets. Any other import replaces the state and is
    /// discovered again. That includes a network in RefreshFailed: rediscovery
    /// is the path that repairs it, so it is not short-circuited here.
    fn merge_reimport(
        &self,
        existing: StackResourceState,
        imported: StackResourceState,
        ctx: &ImportContext<'_>,
    ) -> Result<StackResourceState> {
        let imported_network = network_controller(&imported, ctx, "re-imported")?;
        if !matches!(
            imported_network.state,
            AwsNetworkState::DiscoveringSubnetFailureDomains | AwsNetworkState::CreateStart
        ) {
            return Ok(imported);
        }
        let existing_network = network_controller(&existing, ctx, "existing")?;
        // A network saved before the zone map existed is Ready with an empty
        // map; it must go through discovery to get one.
        if !matches!(existing_network.state, AwsNetworkState::Ready)
            || existing_network.subnets_by_failure_domain.is_empty()
            || !same_network_mode(&existing, &imported)
            || !names_same_network(&imported_network, &existing_network)
        {
            return Ok(imported);
        }

        let merged = if imported_network.vpc_id.is_some() {
            // Setup named the network: its identifiers stay authoritative and
            // only the discovered zone mapping carries over.
            AwsNetworkController {
                state: AwsNetworkState::Ready,
                availability_zones: existing_network.availability_zones,
                subnets_by_failure_domain: existing_network.subnets_by_failure_domain,
                ..imported_network
            }
        } else {
            // Use-default: setup names nothing, everything was discovered.
            existing_network
        };
        make_imported_state(merged, ctx)
    }
}

fn network_controller(
    state: &StackResourceState,
    ctx: &ImportContext<'_>,
    which: &str,
) -> Result<AwsNetworkController> {
    let value = state.internal_state.clone().ok_or_else(|| {
        AlienError::new(CoreErrorData::JsonDeserializationFailed {
            reason: format!(
                "{which} network '{}' has no controller state",
                ctx.resource_id
            ),
        })
    })?;
    serde_json::from_value(value).into_alien_error().context(
        CoreErrorData::JsonDeserializationFailed {
            reason: format!(
                "{which} network '{}' has unreadable controller state",
                ctx.resource_id
            ),
        },
    )
}

/// True when the installed and requested network settings are the same mode
/// (create, use-existing or use-default). Use-existing and use-default imports
/// both mark the VPC as setup-owned, so the controller state alone cannot tell
/// a mode switch apart.
fn same_network_mode(existing: &StackResourceState, imported: &StackResourceState) -> bool {
    match (
        existing.config.downcast_ref::<Network>(),
        imported.config.downcast_ref::<Network>(),
    ) {
        (Some(existing), Some(imported)) => {
            std::mem::discriminant(&existing.settings) == std::mem::discriminant(&imported.settings)
        }
        _ => false,
    }
}

/// True when the import names the VPC and subnets the existing state describes,
/// in any order. Use-default imports name no VPC and no subnets; they match the
/// default VPC the controller discovered.
fn names_same_network(imported: &AwsNetworkController, existing: &AwsNetworkController) -> bool {
    let ids = |ids: &[String]| ids.iter().cloned().collect::<BTreeSet<_>>();
    imported.is_byo_vpc == existing.is_byo_vpc
        && match &imported.vpc_id {
            Some(vpc_id) => {
                existing.vpc_id.as_ref() == Some(vpc_id)
                    && ids(&imported.public_subnet_ids) == ids(&existing.public_subnet_ids)
                    && ids(&imported.private_subnet_ids) == ids(&existing.private_subnet_ids)
            }
            None => imported.public_subnet_ids.is_empty() && imported.private_subnet_ids.is_empty(),
        }
}

#[cfg(test)]
mod tests {
    use alien_core::{
        import::{data::AwsNetworkImportData, ImportContext},
        AwsManagementConfig, ManagementConfig, Network, Platform, Resource, ResourceEntry,
        ResourceLifecycle, ResourceRef, StackSettings,
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
            enabled_when: None,
        }
    }

    fn empty_default_import_data() -> AwsNetworkImportData {
        AwsNetworkImportData {
            vpc_id: None,
            cidr_block: None,
            internet_gateway_id: None,
            nat_gateway_id: None,
            eip_allocation_id: None,
            egress_public_ips: Vec::new(),
            sandbox_network: None,
            public_subnet_ids: Vec::new(),
            private_subnet_ids: Vec::new(),
            public_route_table_id: None,
            private_route_table_id: None,
            security_group_id: None,
            availability_zones: Vec::new(),
            subnets_by_failure_domain: Default::default(),
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
    fn import_records_stack_authored_dependencies() {
        let settings = StackSettings {
            network: Some(NetworkSettings::UseDefault),
            ..StackSettings::default()
        };
        let mut entry = network_entry();
        entry
            .dependencies
            .push(ResourceRef::new("service-activation".into(), "bootstrap"));

        let imported = AwsNetworkImporter
            .import(
                empty_default_import_data(),
                &import_context(&settings, &entry),
            )
            .expect("network import should succeed");

        assert_eq!(imported.dependencies, entry.combined_dependencies());
    }

    #[test]
    fn imported_network_without_domain_map_discovers_before_becoming_ready() {
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

        assert_eq!(imported.status, ResourceStatus::Provisioning);
        let internal = imported
            .internal_state
            .expect("imported network should have controller state");
        assert_eq!(internal["state"], "discoveringSubnetFailureDomains");
        assert_eq!(internal["isByoVpc"], true);
    }

    fn entry_with(settings: &StackSettings) -> ResourceEntry {
        let mut entry = network_entry();
        entry.config = Resource::new(
            Network::new("default-network".to_string())
                .settings(
                    settings
                        .network
                        .clone()
                        .expect("test settings name a network"),
                )
                .build(),
        );
        entry
    }

    fn create_settings() -> StackSettings {
        StackSettings {
            network: Some(NetworkSettings::Create {
                cidr: None,
                availability_zones: 2,
            }),
            ..StackSettings::default()
        }
    }

    fn byo_settings() -> StackSettings {
        StackSettings {
            network: Some(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec!["subnet-pub-a".to_string(), "subnet-pub-b".to_string()],
                private_subnet_ids: vec!["subnet-priv-a".to_string(), "subnet-priv-b".to_string()],
                security_group_ids: vec!["sg-123".to_string()],
            }),
            ..StackSettings::default()
        }
    }

    /// What a CloudFormation stack hands off for a named VPC: IDs, no zone map.
    fn named_network_data(is_byo_vpc: bool) -> AwsNetworkImportData {
        let mut data = empty_default_import_data();
        data.vpc_id = Some("vpc-123".to_string());
        data.public_subnet_ids = vec!["subnet-pub-a".to_string(), "subnet-pub-b".to_string()];
        data.private_subnet_ids = vec!["subnet-priv-a".to_string(), "subnet-priv-b".to_string()];
        data.security_group_id = Some("sg-123".to_string());
        data.nat_gateway_id = (!is_byo_vpc).then(|| "nat-123".to_string());
        data.is_byo_vpc = is_byo_vpc;
        data
    }

    fn discovered_map(
    ) -> std::collections::BTreeMap<String, alien_core::aws::AwsFailureDomainSubnets> {
        ["a", "b"]
            .into_iter()
            .map(|zone| {
                (
                    format!("us-east-2{zone}"),
                    alien_core::aws::AwsFailureDomainSubnets {
                        public_subnet_ids: vec![format!("subnet-pub-{zone}")],
                        private_subnet_ids: vec![format!("subnet-priv-{zone}")],
                    },
                )
            })
            .collect()
    }

    /// The state the controller's discovery step leaves behind: Ready with the map.
    fn discovered_network(
        settings: &StackSettings,
        mut data: AwsNetworkImportData,
    ) -> StackResourceState {
        data.subnets_by_failure_domain = discovered_map();
        let entry = entry_with(settings);
        let state = AwsNetworkImporter
            .import(data, &import_context(settings, &entry))
            .expect("discovered network state should build");
        assert_eq!(state.status, ResourceStatus::Running);
        state
    }

    fn rerun(
        settings: &StackSettings,
        existing: StackResourceState,
        data: AwsNetworkImportData,
    ) -> (StackResourceState, serde_json::Value) {
        let entry = entry_with(settings);
        let ctx = import_context(settings, &entry);
        let reimported = AwsNetworkImporter
            .import(data, &ctx)
            .expect("network import should succeed");
        let merged = AwsNetworkImporter
            .merge_reimport(existing, reimported, &ctx)
            .expect("re-import should merge");
        let internal = merged
            .internal_state
            .clone()
            .expect("merged network should have controller state");
        (merged, internal)
    }

    #[test]
    fn rerun_keeps_zones_but_takes_changed_setup_ids_from_the_import() {
        let settings = create_settings();
        let existing = discovered_network(&settings, named_network_data(false));
        let mut changed = named_network_data(false);
        changed.security_group_id = Some("sg-replaced".to_string());
        changed.nat_gateway_id = Some("nat-replaced".to_string());
        changed.egress_public_ips = vec!["203.0.113.20".to_string()];
        let sandbox_network = alien_core::import::data::AwsSandboxNetwork {
            sandbox_subnet_ids: vec!["subnet-isolated".into()],
            gateway_subnet_ids: vec!["subnet-gateway".into()],
            sandbox_cidrs: vec!["10.0.6.0/24".into()],
            gateway_cidrs: vec!["10.0.9.0/24".into()],
            workload_cidrs: vec!["10.0.3.0/24".into()],
            vpc_cidrs: vec!["10.0.0.0/16".into(), "198.18.0.0/16".into()],
        };
        changed.sandbox_network = Some(sandbox_network.clone());

        let (merged, internal) = rerun(&settings, existing, changed);

        assert_eq!(merged.status, ResourceStatus::Running);
        assert_eq!(internal["state"], "ready");
        assert_eq!(internal["securityGroupId"], "sg-replaced");
        assert_eq!(internal["natGatewayId"], "nat-replaced");
        let controller: AwsNetworkController = serde_json::from_value(internal.clone()).unwrap();
        assert_eq!(controller.sandbox_network, Some(sandbox_network));
        let outputs = serde_json::to_value(merged.outputs.expect("ready network outputs"))
            .expect("serialize network outputs");
        assert_eq!(
            outputs["egressPublicIps"],
            serde_json::json!(["203.0.113.20"])
        );
        assert_eq!(
            internal["subnetsByFailureDomain"],
            serde_json::to_value(discovered_map()).unwrap()
        );
    }

    #[test]
    fn rerun_listing_the_same_subnets_in_another_order_keeps_zones() {
        let settings = create_settings();
        let existing = discovered_network(&settings, named_network_data(false));
        let mut reordered = named_network_data(false);
        reordered.public_subnet_ids.reverse();
        reordered.private_subnet_ids.reverse();

        let (merged, internal) = rerun(&settings, existing, reordered);

        assert_eq!(merged.status, ResourceStatus::Running);
        assert_eq!(internal["state"], "ready");
        assert_eq!(
            internal["subnetsByFailureDomain"],
            serde_json::to_value(discovered_map()).unwrap()
        );
    }

    /// Use-existing and use-default imports are both setup-owned VPCs, so only the
    /// network settings show the switch. The customer VPC must not be kept.
    #[test]
    fn switching_use_existing_to_use_default_rediscovers_the_default_vpc() {
        let existing = discovered_network(&byo_settings(), named_network_data(true));
        let use_default = StackSettings {
            network: Some(NetworkSettings::UseDefault),
            ..StackSettings::default()
        };

        let (merged, internal) = rerun(&use_default, existing, empty_default_import_data());

        assert_eq!(merged.status, ResourceStatus::Provisioning);
        assert_eq!(internal["state"], "createStart");
        assert_eq!(internal["vpcId"], serde_json::Value::Null);
        assert_eq!(internal["subnetsByFailureDomain"], serde_json::json!({}));
    }

    #[test]
    fn rerun_over_a_network_still_discovering_takes_the_import() {
        let settings = byo_settings();
        let entry = entry_with(&settings);
        let existing = AwsNetworkImporter
            .import(named_network_data(true), &import_context(&settings, &entry))
            .expect("network import should succeed");
        assert_eq!(existing.status, ResourceStatus::Provisioning);

        let (merged, internal) = rerun(&settings, existing, named_network_data(true));

        assert_eq!(merged.status, ResourceStatus::Provisioning);
        assert_eq!(internal["state"], "discoveringSubnetFailureDomains");
    }

    /// Networks saved before the zone map existed load as Ready with an empty map.
    /// A setup rerun is how they get one, so it must still discover.
    #[test]
    fn rerun_over_a_ready_network_without_a_zone_map_rediscovers() {
        let settings = byo_settings();
        let mut existing = discovered_network(&settings, named_network_data(true));
        existing
            .internal_state
            .as_mut()
            .expect("network controller state")["subnetsByFailureDomain"] = serde_json::json!({});

        let (merged, internal) = rerun(&settings, existing, named_network_data(true));

        assert_eq!(merged.status, ResourceStatus::Provisioning);
        assert_eq!(internal["state"], "discoveringSubnetFailureDomains");
    }

    /// Use-default setup hands off nothing; the controller discovers the default
    /// VPC and its zones. A setup rerun must keep them instead of restarting
    /// discovery under dependents that are already running.
    #[test]
    fn use_default_reimport_keeps_the_discovered_default_vpc() {
        let settings = StackSettings {
            network: Some(NetworkSettings::UseDefault),
            ..StackSettings::default()
        };
        let entry = network_entry();
        let ctx = import_context(&settings, &entry);
        let mut discovered = empty_default_import_data();
        discovered.vpc_id = Some("vpc-default".to_string());
        discovered.public_subnet_ids = vec!["subnet-default-a".to_string()];
        discovered.subnets_by_failure_domain.insert(
            "us-east-2a".to_string(),
            alien_core::aws::AwsFailureDomainSubnets {
                public_subnet_ids: vec!["subnet-default-a".to_string()],
                private_subnet_ids: Vec::new(),
            },
        );
        let existing = AwsNetworkImporter
            .import(discovered, &ctx)
            .expect("discovered network state should build");
        assert_eq!(existing.status, ResourceStatus::Running);
        let reimported = AwsNetworkImporter
            .import(empty_default_import_data(), &ctx)
            .expect("network import should succeed");

        let merged = AwsNetworkImporter
            .merge_reimport(existing, reimported, &ctx)
            .expect("re-import should merge");

        assert_eq!(merged.status, ResourceStatus::Running);
        let internal = merged
            .internal_state
            .expect("merged network should have controller state");
        assert_eq!(internal["state"], "ready");
        assert_eq!(internal["vpcId"], "vpc-default");
        assert_eq!(
            internal["subnetsByFailureDomain"]["us-east-2a"]["publicSubnetIds"][0],
            "subnet-default-a"
        );
    }

    #[test]
    fn imported_network_with_exact_domain_map_is_ready() {
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
        data.subnets_by_failure_domain.insert(
            "us-west-2b".to_string(),
            alien_core::aws::AwsFailureDomainSubnets {
                public_subnet_ids: vec!["subnet-public".to_string()],
                private_subnet_ids: vec!["subnet-private".to_string()],
            },
        );
        let entry = network_entry();
        let imported = AwsNetworkImporter
            .import(data, &import_context(&settings, &entry))
            .expect("network import should succeed");

        assert_eq!(imported.status, ResourceStatus::Running);
        let internal = imported
            .internal_state
            .expect("imported network should have controller state");
        assert_eq!(internal["state"], "ready");
        assert_eq!(
            internal["subnetsByFailureDomain"]["us-west-2b"]["privateSubnetIds"][0],
            "subnet-private"
        );
    }

    /// A setup artifact that hands off an existing VPC whose subnet parameters were
    /// all left blank must not import as a ready network with no subnets. The
    /// payload goes through the real import deserializer, and the imported state
    /// is stepped to show it fails locally without asking EC2 for every subnet.
    #[tokio::test]
    async fn blank_only_existing_network_import_fails_instead_of_becoming_ready() {
        use std::sync::Arc;

        use alien_aws_clients::ec2::MockEc2Api;
        use alien_core::Platform;

        use crate::{
            core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider},
            import::ErasedImporter,
        };

        let settings = StackSettings {
            network: Some(NetworkSettings::ByoVpcAws {
                vpc_id: "vpc-123".to_string(),
                public_subnet_ids: vec![],
                private_subnet_ids: vec![],
                security_group_ids: vec![],
            }),
            ..StackSettings::default()
        };
        let entry = network_entry();
        let imported = AwsNetworkImporter
            .import_json(
                serde_json::json!({
                    "vpcId": "vpc-123",
                    "publicSubnetIds": [""],
                    "privateSubnetIds": ["  "],
                    "securityGroupId": "",
                    "availabilityZones": [],
                    "isByoVpc": true
                }),
                &import_context(&settings, &entry),
            )
            .expect("blank IDs deserialize as absent");

        assert_eq!(imported.status, ResourceStatus::Provisioning);
        let internal = imported
            .internal_state
            .expect("imported network should have controller state");
        assert_eq!(internal["state"], "discoveringSubnetFailureDomains");
        assert_eq!(internal["publicSubnetIds"], serde_json::json!([]));
        assert_eq!(internal["privateSubnetIds"], serde_json::json!([]));

        let controller: AwsNetworkController =
            serde_json::from_value(internal).expect("imported controller state deserializes");
        // No EC2 expectations: any call, including an unfiltered DescribeSubnets,
        // fails the test.
        let ec2 = Arc::new(MockEc2Api::new());
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        let mut executor = SingleControllerExecutor::builder()
            .resource(
                Network::new("default-network".to_string())
                    .settings(settings.network.clone().unwrap())
                    .build(),
            )
            .controller(controller)
            .platform(Platform::Aws)
            .stack_settings(settings)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .expect("executor should build");

        let error = executor
            .step()
            .await
            .expect_err("a network with no subnet IDs must not become ready");
        assert!(
            error.to_string().contains("at least one subnet ID"),
            "unexpected error: {error}"
        );
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

    /// The `use-default` import hands off half-finished on purpose: status
    /// `Provisioning`, state `CreateStart`, no VPC id, leaving the controller to
    /// discover the account default VPC. Nothing covered that the two halves
    /// compose, so this pins that stepping the imported state actually reaches
    /// discovery. If it ever stops, every resource depending on the network
    /// stalls behind it and initial setup never completes.
    #[tokio::test]
    async fn imported_use_default_network_reaches_default_vpc_discovery() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        use alien_aws_clients::ec2::MockEc2Api;
        use alien_core::{Platform, ResourceStatus};

        use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};

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

        let controller: AwsNetworkController =
            serde_json::from_value(imported.internal_state.expect("controller state"))
                .expect("controller should deserialize from its imported state");

        let discovered = Arc::new(AtomicBool::new(false));
        let flag = discovered.clone();
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_vpcs().returning(move |_| {
            flag.store(true, Ordering::SeqCst);
            Err(alien_error::AlienError::new(
                alien_client_core::ErrorData::RemoteServiceUnavailable {
                    message: "stop after the lookup; provisioning itself is covered elsewhere"
                        .to_string(),
                },
            ))
        });
        let ec2 = Arc::new(ec2);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));

        let network = Network::new("default-network".to_string())
            .settings(NetworkSettings::UseDefault)
            .build();
        let mut executor = SingleControllerExecutor::builder()
            .resource(network)
            .controller(controller)
            .platform(Platform::Aws)
            .stack_settings(settings)
            .service_provider(Arc::new(provider))
            .build()
            .await
            .expect("executor should build");

        let _ = executor.step().await;
        assert!(
            discovered.load(Ordering::SeqCst),
            "stepping the imported use-default network must reach default-VPC discovery"
        );
    }

    /// The production initial-setup path, not the controller in isolation:
    /// `continue_imported` with the Frozen filter, exactly as `initial_setup.rs`
    /// builds it. A `use-default` network arrives here unfinished by design, so
    /// this run is the only thing that can finish it.
    #[tokio::test]
    async fn initial_setup_drives_the_imported_use_default_network() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        use alien_aws_clients::ec2::MockEc2Api;
        use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt as _};
        use alien_core::{
            ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings,
            InitialSetupAuthority, Platform, Stack, StackState,
        };

        use crate::core::{MockPlatformServiceProvider, StackExecutor};

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

        let network = Network::new("default-network".to_string())
            .settings(NetworkSettings::UseDefault)
            .build();
        let stack = Stack::new("use-default-handoff".to_string())
            .add(network, ResourceLifecycle::Frozen)
            .build();
        let mut state = StackState::new(Platform::Aws);
        state
            .resources
            .insert("default-network".to_string(), imported);

        let discovered = Arc::new(AtomicBool::new(false));
        let flag = discovered.clone();
        let mut ec2 = MockEc2Api::new();
        ec2.expect_describe_vpcs().returning(move |_| {
            flag.store(true, Ordering::SeqCst);
            Err(alien_error::AlienError::new(
                alien_client_core::ErrorData::RemoteServiceUnavailable {
                    message: "stop after the lookup".to_string(),
                },
            ))
        });
        let ec2 = Arc::new(ec2);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));

        let config = DeploymentConfig::builder()
            .stack_settings(settings)
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .external_bindings(ExternalBindings::default())
            .allow_frozen_changes(false)
            .build();

        let executor =
            StackExecutor::builder(&stack, ClientConfig::Aws(Box::new(AwsClientConfig::mock())))
                .deployment_config(&config)
                .service_provider(Arc::new(provider))
                .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
                .lifecycle_filter(vec![ResourceLifecycle::Frozen])
                .step_running_resources(false)
                .build()
                .expect("executor should build");

        let _ = executor.continue_imported(state).await;
        assert!(
            discovered.load(Ordering::SeqCst),
            "initial setup must drive the imported use-default network to default-VPC \
             discovery; without it the network stays Provisioning forever and every \
             resource depending on it stalls behind it"
        );
    }
}
