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
        let needs_subnet_domain_discovery = !needs_default_vpc_discovery
            && data.subnets_by_failure_domain.is_empty()
            && (!data.public_subnet_ids.is_empty() || !data.private_subnet_ids.is_empty());
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
            _internal_stay_count: None,
        };
        if needs_default_vpc_discovery || needs_subnet_domain_discovery {
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
