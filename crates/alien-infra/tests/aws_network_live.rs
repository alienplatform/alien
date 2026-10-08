//! The AWS network controller against real EC2, with faults injected into the EC2 client.
//!
//! Each scenario drives the real `AwsNetworkController` through the real alien EC2 client,
//! wrapped so that a chosen call can fail before it is sent or be sent and then have its
//! response "lost" (the call takes effect, the controller sees an error). Every scenario uses
//! its own resource prefix, ends with a leak check over everything tagged with that prefix
//! (VPCs, subnets, internet gateways, NAT gateways, Elastic IPs, route tables, security
//! groups), and always sweeps what is left, even when an assertion failed.
//!
//! Ignored by default: it creates billed resources (NAT gateways bill hourly; each run keeps
//! them for a few minutes). It needs an account with room for 2 VPCs and 1 Elastic IP in the
//! region, and refuses to run unless `ALIEN_AWS_NETWORK_LIVE_TEST=1`:
//!
//! ```text
//! export ALIEN_AWS_NETWORK_LIVE_TEST=1 AWS_ACCOUNT_ID=... AWS_ACCESS_KEY_ID=... \
//!        AWS_SECRET_ACCESS_KEY=... AWS_SESSION_TOKEN=...   # region: ALIEN_AWS_NETWORK_LIVE_REGION, default eu-west-1
//! cargo test -p alien-infra --all-features --test aws_network_live -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Only resources tagged `deployment=<scenario prefix>` are ever deleted by the sweep.

#![cfg(all(feature = "aws", feature = "test-utils"))]

use std::collections::HashMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alien_aws_clients::ec2::*;
use alien_client_core::{ErrorData as ClientErrorData, Result as ClientResult};
use alien_core::{
    standard_resource_tags, AwsClientConfig, AwsCredentials, ClientConfig, Network,
    NetworkSettings, Platform, ResourceStatus, ALIEN_STACK_TAG_KEY,
};
use alien_error::AlienError;
use alien_infra::controller_test::SingleControllerExecutor;
use alien_infra::{
    AwsNetworkController, DefaultPlatformServiceProvider, MockPlatformServiceProvider,
    PlatformServiceProvider, ResourceController,
};
use async_trait::async_trait;
use futures::FutureExt;
use serde_json::Value;

const NETWORK_ID: &str = "net";

// ─────────────── fault injection ──────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    /// The call is sent and takes effect; the controller gets an error instead of the response.
    LoseResponse,
    /// The call fails without being sent.
    FailBeforeSend,
}

#[derive(Debug, Clone)]
struct Injection {
    operation: &'static str,
    /// 1-based occurrence of `operation` this applies to.
    occurrence: usize,
    fault: Fault,
}

#[derive(Debug, Clone)]
struct CallRecord {
    operation: &'static str,
    at: Duration,
    outcome: String,
}

/// An `Ec2Api` that forwards to the real client, logs every call and applies injections.
#[derive(Debug)]
struct FaultyEc2 {
    inner: Mutex<Arc<dyn Ec2Api>>,
    started: Instant,
    injections: Mutex<Vec<Injection>>,
    counts: Mutex<HashMap<&'static str, usize>>,
    log: Mutex<Vec<CallRecord>>,
}

fn describe_error(error: &AlienError<ClientErrorData>) -> String {
    let mut text = format!("{}: {}", error.code, error.message);
    let mut source = error.source.as_deref();
    while let Some(inner) = source {
        text.push_str(&format!(" <- {}: {}", inner.code, inner.message));
        source = inner.source.as_deref();
    }
    text
}

impl FaultyEc2 {
    fn new(inner: Arc<dyn Ec2Api>) -> Self {
        Self {
            inner: Mutex::new(inner),
            started: Instant::now(),
            injections: Mutex::new(Vec::new()),
            counts: Mutex::new(HashMap::new()),
            log: Mutex::new(Vec::new()),
        }
    }

    fn client(&self) -> Arc<dyn Ec2Api> {
        self.inner.lock().unwrap().clone()
    }

    fn set_client(&self, client: Arc<dyn Ec2Api>) {
        *self.inner.lock().unwrap() = client;
    }

    fn inject(&self, operation: &'static str, occurrence: usize, fault: Fault) {
        self.injections.lock().unwrap().push(Injection {
            operation,
            occurrence,
            fault,
        });
    }

    fn calls(&self, operation: &str) -> Vec<CallRecord> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|record| record.operation == operation)
            .cloned()
            .collect()
    }

    fn log(&self) -> Vec<CallRecord> {
        self.log.lock().unwrap().clone()
    }

    async fn call<T: Send>(
        &self,
        operation: &'static str,
        request: impl Future<Output = ClientResult<T>> + Send,
    ) -> ClientResult<T> {
        let occurrence = {
            let mut counts = self.counts.lock().unwrap();
            let count = counts.entry(operation).or_insert(0);
            *count += 1;
            *count
        };
        let fault = self
            .injections
            .lock()
            .unwrap()
            .iter()
            .find(|injection| {
                injection.operation == operation && injection.occurrence == occurrence
            })
            .map(|injection| injection.fault);
        let injected_error = || {
            AlienError::new(ClientErrorData::HttpRequestFailed {
                message: format!("injected fault on {operation} #{occurrence}"),
            })
        };

        let (result, outcome) = match fault {
            Some(Fault::FailBeforeSend) => (Err(injected_error()), "not sent".to_string()),
            Some(Fault::LoseResponse) => {
                let outcome = match request.await {
                    Ok(_) => "sent, succeeded, response dropped".to_string(),
                    Err(error) => format!("sent, failed: {}", describe_error(&error)),
                };
                (Err(injected_error()), outcome)
            }
            None => {
                let result = request.await;
                let outcome = match &result {
                    Ok(_) => "ok".to_string(),
                    Err(error) => format!("error: {}", describe_error(error)),
                };
                (result, outcome)
            }
        };
        let record = CallRecord {
            operation,
            at: self.started.elapsed(),
            outcome,
        };
        println!(
            "    [{:>7.1}s] {operation} #{occurrence}: {}{}",
            record.at.as_secs_f64(),
            record.outcome,
            fault
                .map(|f| format!(" (injected {f:?})"))
                .unwrap_or_default()
        );
        self.log.lock().unwrap().push(record);
        result
    }
}

/// Implements `Ec2Api` for `FaultyEc2`, routing every method through `FaultyEc2::call`.
macro_rules! faulty_ec2_api {
    ($( $name:ident ( $( $arg:ident : $ty:ty ),* ) -> $ret:ty; )*) => {
        #[async_trait]
        impl Ec2Api for FaultyEc2 {
            $(
                async fn $name(&self, $( $arg: $ty ),*) -> ClientResult<$ret> {
                    self.call(stringify!($name), self.client().$name($( $arg ),*)).await
                }
            )*
        }
    };
}

faulty_ec2_api! {
    describe_vpcs(request: DescribeVpcsRequest) -> DescribeVpcsResponse;
    describe_vpc_attribute(request: DescribeVpcAttributeRequest) -> DescribeVpcAttributeResponse;
    create_vpc(request: CreateVpcRequest) -> CreateVpcResponse;
    delete_vpc(vpc_id: &str) -> ();
    modify_vpc_attribute(request: ModifyVpcAttributeRequest) -> ();
    describe_subnets(request: DescribeSubnetsRequest) -> DescribeSubnetsResponse;
    create_subnet(request: CreateSubnetRequest) -> CreateSubnetResponse;
    delete_subnet(subnet_id: &str) -> ();
    create_internet_gateway(request: CreateInternetGatewayRequest) -> CreateInternetGatewayResponse;
    delete_internet_gateway(internet_gateway_id: &str) -> ();
    attach_internet_gateway(request: AttachInternetGatewayRequest) -> ();
    detach_internet_gateway(request: DetachInternetGatewayRequest) -> ();
    describe_internet_gateways(request: DescribeInternetGatewaysRequest) -> DescribeInternetGatewaysResponse;
    create_nat_gateway(request: CreateNatGatewayRequest) -> CreateNatGatewayResponse;
    delete_nat_gateway(nat_gateway_id: &str) -> DeleteNatGatewayResponse;
    describe_nat_gateways(request: DescribeNatGatewaysRequest) -> DescribeNatGatewaysResponse;
    allocate_address(request: AllocateAddressRequest) -> AllocateAddressResponse;
    describe_addresses() -> DescribeAddressesResponse;
    release_address(allocation_id: &str) -> ();
    describe_route_tables(request: DescribeRouteTablesRequest) -> DescribeRouteTablesResponse;
    create_route_table(request: CreateRouteTableRequest) -> CreateRouteTableResponse;
    delete_route_table(route_table_id: &str) -> ();
    create_route(request: CreateRouteRequest) -> ();
    delete_route(request: DeleteRouteRequest) -> ();
    associate_route_table(request: AssociateRouteTableRequest) -> AssociateRouteTableResponse;
    disassociate_route_table(association_id: &str) -> ();
    describe_security_groups(request: DescribeSecurityGroupsRequest) -> DescribeSecurityGroupsResponse;
    describe_network_interfaces(request: DescribeNetworkInterfacesRequest) -> DescribeNetworkInterfacesResponse;
    create_network_interface(request: CreateNetworkInterfaceRequest) -> CreateNetworkInterfaceResponse;
    delete_network_interface(network_interface_id: &str) -> ();
    create_security_group(request: CreateSecurityGroupRequest) -> CreateSecurityGroupResponse;
    delete_security_group(group_id: &str) -> ();
    authorize_security_group_ingress(request: AuthorizeSecurityGroupIngressRequest) -> ();
    authorize_security_group_egress(request: AuthorizeSecurityGroupEgressRequest) -> ();
    revoke_security_group_ingress(request: RevokeSecurityGroupIngressRequest) -> ();
    revoke_security_group_egress(request: RevokeSecurityGroupEgressRequest) -> ();
    describe_availability_zones(request: DescribeAvailabilityZonesRequest) -> DescribeAvailabilityZonesResponse;
    describe_instance_type_offerings(request: DescribeInstanceTypeOfferingsRequest) -> DescribeInstanceTypeOfferingsResponse;
    describe_images(request: DescribeImagesRequest) -> DescribeImagesResponse;
    terminate_instances(instance_ids: Vec<String>) -> TerminateInstancesResponse;
    describe_instances(request: DescribeInstancesRequest) -> DescribeInstancesResponse;
    create_volume(request: CreateVolumeRequest) -> CreateVolumeResponse;
    modify_volume(request: ModifyVolumeRequest) -> ModifyVolumeResponse;
    describe_volumes_modifications(request: DescribeVolumesModificationsRequest) -> DescribeVolumesModificationsResponse;
    delete_volume(volume_id: &str) -> ();
    describe_volumes(request: DescribeVolumesRequest) -> DescribeVolumesResponse;
    attach_volume(request: AttachVolumeRequest) -> AttachVolumeResponse;
    detach_volume(request: DetachVolumeRequest) -> DetachVolumeResponse;
    create_snapshot(request: CreateSnapshotRequest) -> Snapshot;
    describe_snapshots(request: DescribeSnapshotsRequest) -> DescribeSnapshotsResponse;
    delete_snapshot(snapshot_id: &str) -> ();
    create_tags(request: CreateTagsRequest) -> ();
    delete_tags(request: DeleteTagsRequest) -> ();
    create_launch_template(request: CreateLaunchTemplateRequest) -> CreateLaunchTemplateResponse;
    create_launch_template_version(request: CreateLaunchTemplateVersionRequest) -> CreateLaunchTemplateVersionResponse;
    delete_launch_template(request: DeleteLaunchTemplateRequest) -> DeleteLaunchTemplateResponse;
    describe_launch_templates(request: DescribeLaunchTemplatesRequest) -> DescribeLaunchTemplatesResponse;
    get_console_output(instance_id: String) -> GetConsoleOutputResponse;
}

// ─────────────── live environment ─────────────────────────────

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set"))
}

fn aws_config() -> AwsClientConfig {
    assert_eq!(
        std::env::var("ALIEN_AWS_NETWORK_LIVE_TEST").as_deref(),
        Ok("1"),
        "set ALIEN_AWS_NETWORK_LIVE_TEST=1 to run the live AWS network tests; they create billed resources"
    );
    AwsClientConfig {
        account_id: env("AWS_ACCOUNT_ID"),
        region: std::env::var("ALIEN_AWS_NETWORK_LIVE_REGION")
            .unwrap_or_else(|_| "eu-west-1".to_string()),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: env("AWS_ACCESS_KEY_ID"),
            secret_access_key: env("AWS_SECRET_ACCESS_KEY"),
            session_token: std::env::var("AWS_SESSION_TOKEN").ok(),
        },
        service_overrides: None,
    }
}

struct Live {
    prefix: String,
    started: Instant,
    real: Arc<dyn Ec2Api>,
    faulty: Arc<FaultyEc2>,
    executor: SingleControllerExecutor,
}

impl Live {
    async fn new(scenario: &str, cidr: &str) -> Self {
        let config = aws_config();
        let suffix = &uuid::Uuid::new_v4().simple().to_string()[..6];
        let prefix = format!("alnet-{scenario}-{suffix}");
        let real_provider = DefaultPlatformServiceProvider::default();
        let real = real_provider
            .get_aws_ec2_client(&config)
            .await
            .expect("real EC2 client");
        let quotas = real_provider
            .get_aws_service_quotas_client(&config)
            .await
            .expect("real Service Quotas client");
        let faulty = Arc::new(FaultyEc2::new(real.clone()));
        let ec2: Arc<dyn Ec2Api> = faulty.clone();

        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(ec2.clone()));
        provider
            .expect_get_aws_service_quotas_client()
            .returning(move |_| Ok(quotas.clone()));

        let network = Network::new(NETWORK_ID.to_string())
            .settings(NetworkSettings::Create {
                cidr: Some(cidr.to_string()),
                availability_zones: 2,
            })
            .build();
        let executor = SingleControllerExecutor::builder()
            .resource(network)
            .controller(AwsNetworkController::default())
            .platform(Platform::Aws)
            .client_config(ClientConfig::Aws(Box::new(config)))
            .service_provider(Arc::new(provider))
            .resource_prefix(prefix.clone())
            .build()
            .await
            .expect("executor builds");
        println!("== scenario {scenario}: prefix {prefix}, cidr {cidr}");
        Self {
            prefix,
            started: Instant::now(),
            real,
            faulty,
            executor,
        }
    }

    fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn controller(&self) -> Value {
        serde_json::to_value(
            self.executor
                .internal_state::<AwsNetworkController>()
                .expect("network controller"),
        )
        .expect("controller serializes")
    }

    fn state(&self) -> String {
        self.controller()["state"]
            .as_str()
            .expect("state")
            .to_string()
    }

    /// Steps the controller until `done` holds, sleeping each suggested delay (at most
    /// `max_delay`). A failed step is returned to the caller, which decides whether to
    /// retry it (another call), as the executor does.
    async fn drive(
        &mut self,
        label: &str,
        max_delay: Duration,
        done: impl Fn(&Self) -> bool,
    ) -> Result<(), AlienError<alien_infra::ErrorData>> {
        for _ in 0..400 {
            if done(self) {
                println!(
                    "  [{:>7.1}s] {label}: reached {}",
                    self.elapsed(),
                    self.state()
                );
                return Ok(());
            }
            match self.executor.step().await {
                Ok(result) => {
                    let delay = result
                        .suggested_delay
                        .unwrap_or(Duration::from_millis(50))
                        .min(max_delay);
                    tokio::time::sleep(delay).await;
                }
                Err(error) => {
                    println!(
                        "  [{:>7.1}s] {label}: step failed in {}: {}",
                        self.elapsed(),
                        self.state(),
                        error
                    );
                    return Err(error);
                }
            }
        }
        panic!(
            "{label}: did not finish in 400 steps; state {}",
            self.state()
        );
    }

    /// Like `drive`, but retries failed steps the way the executor does and returns the
    /// errors it saw.
    async fn drive_retrying(
        &mut self,
        label: &str,
        max_delay: Duration,
        done: impl Fn(&Self) -> bool,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        loop {
            match self.drive(label, max_delay, &done).await {
                Ok(()) => return errors,
                Err(error) => {
                    errors.push(format!("{}: {error}", self.state()));
                    assert!(
                        errors.len() <= 5,
                        "{label}: too many failed steps: {errors:?}"
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn create_until(&mut self, state: &'static str) -> Vec<String> {
        self.drive_retrying(
            &format!("create until {state}"),
            Duration::from_secs(15),
            |live| live.state() == state,
        )
        .await
    }

    async fn create_to_running(&mut self) -> Vec<String> {
        self.drive_retrying("create", Duration::from_secs(15), |live| {
            live.executor.status() == ResourceStatus::Running
        })
        .await
    }

    async fn delete_to_deleted(&mut self) {
        self.executor.delete().expect("delete transition");
        let start = self.elapsed();
        let errors = self
            .drive_retrying("delete", Duration::from_secs(15), |live| {
                live.executor.status() == ResourceStatus::Deleted
            })
            .await;
        assert!(errors.is_empty(), "delete failed steps: {errors:?}");
        println!("  delete took {:.1}s", self.elapsed() - start);
        let controller = self.controller();
        for key in [
            "vpcId",
            "internetGatewayId",
            "natGatewayId",
            "eipAllocationId",
            "publicRouteTableId",
            "privateRouteTableId",
            "securityGroupId",
        ] {
            assert!(
                controller[key].is_null(),
                "{key} still recorded: {controller}"
            );
        }
        assert_eq!(controller["publicSubnetIds"], serde_json::json!([]));
        assert_eq!(controller["privateSubnetIds"], serde_json::json!([]));
    }

    async fn inventory(&self) -> Inventory {
        inventory(self.real.as_ref(), &self.prefix).await
    }
}

// ─────────────── inventory and sweep ──────────────────────────

/// Everything carrying `deployment=<prefix>`; NAT gateways in `deleted` are left out.
#[derive(Debug, Default)]
struct Inventory {
    vpcs: Vec<String>,
    subnets: Vec<String>,
    internet_gateways: Vec<String>,
    nat_gateways: Vec<(String, String)>,
    elastic_ips: Vec<String>,
    route_tables: Vec<String>,
    security_groups: Vec<String>,
    network_interfaces: Vec<String>,
}

impl Inventory {
    fn is_empty(&self) -> bool {
        self.vpcs.is_empty()
            && self.subnets.is_empty()
            && self.internet_gateways.is_empty()
            && self.nat_gateways.is_empty()
            && self.elastic_ips.is_empty()
            && self.route_tables.is_empty()
            && self.security_groups.is_empty()
            && self.network_interfaces.is_empty()
    }
}

fn prefix_filter(prefix: &str) -> Vec<Filter> {
    vec![Filter {
        name: format!("tag:{ALIEN_STACK_TAG_KEY}"),
        values: vec![prefix.to_string()],
    }]
}

fn tagged(tags: Option<&TagSet>, key: &str, value: &str) -> bool {
    tags.is_some_and(|set| {
        set.items
            .iter()
            .any(|tag| tag.key == key && tag.value == value)
    })
}

async fn inventory(ec2: &dyn Ec2Api, prefix: &str) -> Inventory {
    let vpcs = ec2
        .describe_vpcs(
            DescribeVpcsRequest::builder()
                .filters(prefix_filter(prefix))
                .build(),
        )
        .await
        .expect("describe VPCs");
    let subnets = ec2
        .describe_subnets(
            DescribeSubnetsRequest::builder()
                .filters(prefix_filter(prefix))
                .build(),
        )
        .await
        .expect("describe subnets");
    let gateways = ec2
        .describe_internet_gateways(
            DescribeInternetGatewaysRequest::builder()
                .filters(prefix_filter(prefix))
                .build(),
        )
        .await
        .expect("describe internet gateways");
    let nats = ec2
        .describe_nat_gateways(
            DescribeNatGatewaysRequest::builder()
                .filters(prefix_filter(prefix))
                .build(),
        )
        .await
        .expect("describe NAT gateways");
    let addresses = ec2.describe_addresses().await.expect("describe addresses");
    let interfaces = ec2
        .describe_network_interfaces(
            DescribeNetworkInterfacesRequest::builder()
                .filters(prefix_filter(prefix))
                .build(),
        )
        .await
        .expect("describe network interfaces");
    let route_tables = ec2
        .describe_route_tables(
            DescribeRouteTablesRequest::builder()
                .filters(prefix_filter(prefix))
                .build(),
        )
        .await
        .expect("describe route tables");
    let groups = ec2
        .describe_security_groups(
            DescribeSecurityGroupsRequest::builder()
                .filters(prefix_filter(prefix))
                .build(),
        )
        .await
        .expect("describe security groups");

    Inventory {
        vpcs: vpcs
            .vpc_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|vpc| vpc.vpc_id)
            .collect(),
        subnets: subnets
            .subnet_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|subnet| subnet.subnet_id)
            .collect(),
        internet_gateways: gateways
            .internet_gateway_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|gateway| gateway.internet_gateway_id)
            .collect(),
        nat_gateways: nats
            .nat_gateway_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter(|nat| nat.state.as_deref() != Some("deleted"))
            .filter_map(|nat| Some((nat.nat_gateway_id?, nat.state.unwrap_or_default())))
            .collect(),
        elastic_ips: addresses
            .addresses_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter(|address| tagged(address.tag_set.as_ref(), ALIEN_STACK_TAG_KEY, prefix))
            .filter_map(|address| address.allocation_id)
            .collect(),
        route_tables: route_tables
            .route_table_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|table| table.route_table_id)
            .collect(),
        security_groups: groups
            .security_group_info
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|group| group.group_id)
            .collect(),
        network_interfaces: interfaces
            .network_interface_set
            .map(|set| set.items)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|interface| interface.network_interface_id)
            .collect(),
    }
}

/// Deletes everything tagged with `prefix`, in dependency order, and returns what it found.
/// Errors are printed and retried a few times; whatever survives is in the final leak check.
async fn sweep(ec2: &dyn Ec2Api, prefix: &str) -> Inventory {
    let found = inventory(ec2, prefix).await;
    if found.is_empty() {
        return found;
    }
    println!("  sweep {prefix}: found {found:?}");
    for (nat, _) in &found.nat_gateways {
        if let Err(error) = ec2.delete_nat_gateway(nat).await {
            println!("  sweep: delete NAT {nat}: {}", describe_error(&error));
        }
    }
    for _ in 0..60 {
        if inventory(ec2, prefix).await.nat_gateways.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
    for round in 0..6 {
        let left = inventory(ec2, prefix).await;
        if left.is_empty() {
            break;
        }
        if round > 0 {
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
        for eip in &left.elastic_ips {
            if let Err(error) = ec2.release_address(eip).await {
                println!("  sweep: release {eip}: {}", describe_error(&error));
            }
        }
        for interface in &left.network_interfaces {
            if let Err(error) = ec2.delete_network_interface(interface).await {
                println!(
                    "  sweep: delete ENI {interface}: {}",
                    describe_error(&error)
                );
            }
        }
        for group in &left.security_groups {
            if let Err(error) = ec2.delete_security_group(group).await {
                println!("  sweep: delete SG {group}: {}", describe_error(&error));
            }
        }
        for subnet in &left.subnets {
            if let Err(error) = ec2.delete_subnet(subnet).await {
                println!(
                    "  sweep: delete subnet {subnet}: {}",
                    describe_error(&error)
                );
            }
        }
        for table in &left.route_tables {
            if let Err(error) = ec2.delete_route_table(table).await {
                println!(
                    "  sweep: delete route table {table}: {}",
                    describe_error(&error)
                );
            }
        }
        for gateway in &left.internet_gateways {
            for vpc in &left.vpcs {
                let _ = ec2
                    .detach_internet_gateway(
                        DetachInternetGatewayRequest::builder()
                            .internet_gateway_id(gateway.clone())
                            .vpc_id(vpc.clone())
                            .build(),
                    )
                    .await;
            }
            if let Err(error) = ec2.delete_internet_gateway(gateway).await {
                println!("  sweep: delete IGW {gateway}: {}", describe_error(&error));
            }
        }
        for vpc in &left.vpcs {
            if let Err(error) = ec2.delete_vpc(vpc).await {
                println!("  sweep: delete VPC {vpc}: {}", describe_error(&error));
            }
        }
    }
    found
}

/// Runs a scenario, then always sweeps its prefix. A scenario that passed must have left
/// nothing for the sweep; a panicking one is re-raised after the sweep.
async fn scenario<F, Fut>(name: &str, cidr: &str, body: F)
where
    F: FnOnce(Live) -> Fut,
    Fut: Future<Output = Live>,
{
    let live = Live::new(name, cidr).await;
    let prefix = live.prefix.clone();
    let real = live.real.clone();
    let started = live.started;
    let outcome = AssertUnwindSafe(body(live)).catch_unwind().await;
    let leftovers = sweep(real.as_ref(), &prefix).await;
    let remaining = inventory(real.as_ref(), &prefix).await;
    println!(
        "== scenario {name} finished in {:.1}s",
        started.elapsed().as_secs_f64()
    );
    assert!(
        remaining.is_empty(),
        "{prefix}: the sweep could not delete {remaining:?}; delete them by hand"
    );
    match outcome {
        Err(panic) => std::panic::resume_unwind(panic),
        Ok(live) => {
            assert!(
                leftovers.is_empty(),
                "{prefix}: the controller's delete leaked {leftovers:?}"
            );
            print_summary(&live);
        }
    }
}

fn print_summary(live: &Live) {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for record in live.faulty.log() {
        match counts.iter_mut().find(|(op, _)| *op == record.operation) {
            Some((_, count)) => *count += 1,
            None => counts.push((record.operation, 1)),
        }
    }
    println!("  calls: {counts:?}");
}

fn assert_full_network(inventory: &Inventory) {
    assert_eq!(inventory.vpcs.len(), 1, "{inventory:?}");
    assert_eq!(inventory.subnets.len(), 4, "{inventory:?}");
    assert_eq!(inventory.internet_gateways.len(), 1, "{inventory:?}");
    assert_eq!(inventory.nat_gateways.len(), 1, "{inventory:?}");
    assert_eq!(inventory.nat_gateways[0].1, "available", "{inventory:?}");
    assert_eq!(inventory.elastic_ips.len(), 1, "{inventory:?}");
    assert_eq!(inventory.route_tables.len(), 2, "{inventory:?}");
    assert_eq!(inventory.security_groups.len(), 1, "{inventory:?}");
}

fn assert_recorded_matches(live: &Live, inventory: &Inventory) {
    let controller = live.controller();
    assert_eq!(
        controller["vpcId"].as_str(),
        inventory.vpcs.first().map(String::as_str)
    );
    if let Some(gateway) = inventory.internet_gateways.first() {
        assert_eq!(
            controller["internetGatewayId"].as_str(),
            Some(gateway.as_str())
        );
    }
    if let Some(eip) = inventory.elastic_ips.first() {
        assert_eq!(controller["eipAllocationId"].as_str(), Some(eip.as_str()));
    }
    if let Some((nat, _)) = inventory.nat_gateways.first() {
        assert_eq!(controller["natGatewayId"].as_str(), Some(nat.as_str()));
    }
    let mut recorded: Vec<String> = controller["publicSubnetIds"]
        .as_array()
        .unwrap()
        .iter()
        .chain(controller["privateSubnetIds"].as_array().unwrap())
        .map(|id| id.as_str().unwrap().to_string())
        .collect();
    recorded.sort();
    let mut existing = inventory.subnets.clone();
    existing.sort();
    assert_eq!(recorded, existing, "recorded subnets differ from AWS");
}

async fn assert_nothing_left(live: &Live) {
    let left = live.inventory().await;
    assert!(left.is_empty(), "left after delete: {left:?}");
}

// ─────────────── scenarios ────────────────────────────────────

/// 1. Create and delete with no faults.
#[tokio::test]
#[ignore = "creates billed AWS resources; needs ALIEN_AWS_NETWORK_LIVE_TEST=1"]
async fn happy_path_create_then_delete_leaves_nothing() {
    scenario("happy", "10.231.0.0/16", |mut live| async move {
        let errors = live.create_to_running().await;
        assert!(errors.is_empty(), "{errors:?}");
        let created = live.inventory().await;
        assert_full_network(&created);
        assert_recorded_matches(&live, &created);
        live.delete_to_deleted().await;
        assert_nothing_left(&live).await;
        live
    })
    .await;
}

/// 2. The `create_vpc` response is lost; the retry adopts that VPC instead of creating another.
#[tokio::test]
#[ignore = "creates billed AWS resources; needs ALIEN_AWS_NETWORK_LIVE_TEST=1"]
async fn lost_create_vpc_response_is_adopted_by_the_retry() {
    scenario("vpc", "10.232.0.0/16", |mut live| async move {
        live.faulty.inject("create_vpc", 1, Fault::LoseResponse);
        let errors = live.create_until("creatingSubnets").await;
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].starts_with("creatingVpc"), "{errors:?}");
        assert_eq!(
            live.faulty.calls("create_vpc").len(),
            1,
            "no second VPC was created"
        );

        let created = live.inventory().await;
        assert_eq!(created.vpcs.len(), 1, "{created:?}");
        assert_eq!(created.internet_gateways.len(), 1, "{created:?}");
        assert_recorded_matches(&live, &created);
        let token = live.controller()["vpcCreateToken"]
            .as_str()
            .expect("token recorded")
            .to_string();
        let tagged_with_token = live
            .real
            .describe_vpcs(
                DescribeVpcsRequest::builder()
                    .filters(vec![Filter {
                        name: "tag:CreateAttempt".to_string(),
                        values: vec![token],
                    }])
                    .build(),
            )
            .await
            .expect("describe VPCs")
            .vpc_set
            .map(|set| set.items)
            .unwrap_or_default();
        assert_eq!(tagged_with_token.len(), 1);

        live.delete_to_deleted().await;
        assert_nothing_left(&live).await;
        live
    })
    .await;
}

/// 3. The `create_subnet` (second subnet) and `create_internet_gateway` responses are lost.
#[tokio::test]
#[ignore = "creates billed AWS resources; needs ALIEN_AWS_NETWORK_LIVE_TEST=1"]
async fn lost_subnet_and_internet_gateway_responses_are_adopted() {
    scenario("subnet-igw", "10.233.0.0/16", |mut live| async move {
        live.faulty
            .inject("create_internet_gateway", 1, Fault::LoseResponse);
        live.faulty.inject("create_subnet", 2, Fault::LoseResponse);
        let errors = live.create_until("allocatingElasticIp").await;
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors[0].starts_with("creatingInternetGateway"),
            "{errors:?}"
        );
        assert!(errors[1].starts_with("creatingSubnets"), "{errors:?}");
        assert_eq!(live.faulty.calls("create_internet_gateway").len(), 1);
        assert_eq!(
            live.faulty.calls("create_subnet").len(),
            4,
            "one call per subnet"
        );

        let created = live.inventory().await;
        assert_eq!(created.vpcs.len(), 1, "{created:?}");
        assert_eq!(created.internet_gateways.len(), 1, "{created:?}");
        assert_eq!(created.subnets.len(), 4, "{created:?}");
        assert_eq!(created.route_tables.len(), 2, "{created:?}");
        assert_recorded_matches(&live, &created);
        assert!(live.controller()["subnetCreateAttempt"].is_null());

        live.delete_to_deleted().await;
        assert_nothing_left(&live).await;
        live
    })
    .await;
}

/// 4. The `allocate_address` and `create_nat_gateway` responses are lost: one Elastic IP and
/// one NAT gateway exist, and delete waits for the NAT gateway before releasing the address.
#[tokio::test]
#[ignore = "creates billed AWS resources; needs ALIEN_AWS_NETWORK_LIVE_TEST=1"]
async fn lost_elastic_ip_and_nat_gateway_responses_leave_one_of_each() {
    scenario("eip-nat", "10.234.0.0/16", |mut live| async move {
        live.faulty
            .inject("allocate_address", 1, Fault::LoseResponse);
        live.faulty
            .inject("create_nat_gateway", 1, Fault::LoseResponse);
        let errors = live.create_to_running().await;
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].starts_with("allocatingElasticIp"), "{errors:?}");
        assert!(errors[1].starts_with("creatingNatGateway"), "{errors:?}");
        assert_eq!(live.faulty.calls("allocate_address").len(), 1);
        // The retry repeats CreateNatGateway with the same client token.
        assert_eq!(live.faulty.calls("create_nat_gateway").len(), 2);

        let created = live.inventory().await;
        assert_full_network(&created);
        assert_recorded_matches(&live, &created);

        // What AWS answers when an address is released while its NAT gateway holds it.
        let eip = created.elastic_ips[0].clone();
        match live.real.release_address(&eip).await {
            Ok(()) => panic!("AWS released {eip} while its NAT gateway was available"),
            Err(error) => println!(
                "  release while the NAT gateway holds the address: {}",
                describe_error(&error)
            ),
        }

        live.delete_to_deleted().await;
        let log = live.faulty.log();
        let position = |op: &str| log.iter().position(|record| record.operation == op);
        let nat_deleted = position("delete_nat_gateway").expect("NAT delete");
        let releases: Vec<&CallRecord> = log
            .iter()
            .filter(|record| record.operation == "release_address")
            .collect();
        assert_eq!(
            releases.len(),
            1,
            "released once, without in-use polls: {releases:?}"
        );
        assert_eq!(releases[0].outcome, "ok");
        let release = position("release_address").unwrap();
        let last_nat_describe = log
            .iter()
            .rposition(|record| record.operation == "describe_nat_gateways")
            .unwrap();
        assert!(nat_deleted < last_nat_describe && last_nat_describe < release);
        println!(
            "  NAT deletion wait: {:.1}s",
            (log[release].at - log[nat_deleted].at).as_secs_f64()
        );
        assert_nothing_left(&live).await;
        live
    })
    .await;
}

/// 5. The create fails after the VPC, subnets and NAT gateway exist (the security group
/// create keeps failing); delete from that state, the entry the executor's replace uses.
#[tokio::test]
#[ignore = "creates billed AWS resources; needs ALIEN_AWS_NETWORK_LIVE_TEST=1"]
async fn delete_after_a_create_failed_late_removes_everything() {
    scenario("late-fail", "10.235.0.0/16", |mut live| async move {
        for occurrence in 1..=3 {
            live.faulty
                .inject("create_security_group", occurrence, Fault::FailBeforeSend);
        }
        let mut errors = Vec::new();
        for _ in 0..3 {
            let error = live
                .drive("create", Duration::from_secs(15), |live| {
                    live.executor.status() == ResourceStatus::Running
                })
                .await
                .expect_err("the security group create fails");
            errors.push(error.to_string());
            assert_eq!(live.state(), "creatingSecurityGroup");
        }
        let created = live.inventory().await;
        assert_eq!(created.nat_gateways.len(), 1, "{created:?}");
        assert_eq!(created.security_groups.len(), 0, "{created:?}");
        assert_recorded_matches(&live, &created);

        live.delete_to_deleted().await;
        assert_nothing_left(&live).await;
        live
    })
    .await;
}

/// 6. A stray security group keeps the VPC in use. Delete keeps the VPC ID and polls, then
/// fails with the ID still recorded; once the dependency is gone the retried delete ends.
#[tokio::test]
#[ignore = "creates billed AWS resources; needs ALIEN_AWS_NETWORK_LIVE_TEST=1"]
async fn delete_with_a_dependency_in_use_keeps_the_vpc_id_until_it_can_delete() {
    scenario("in-use", "10.236.0.0/16", |mut live| async move {
        let errors = live.create_until("allocatingElasticIp").await;
        assert!(errors.is_empty(), "{errors:?}");
        let vpc_id = live.controller()["vpcId"].as_str().unwrap().to_string();
        let stray = live
            .real
            .create_security_group(
                CreateSecurityGroupRequest::builder()
                    .group_name(format!("{}-stray", live.prefix))
                    .description("stray dependency for the live network test".to_string())
                    .vpc_id(vpc_id.clone())
                    .tag_specifications(vec![TagSpecification {
                        resource_type: "security-group".to_string(),
                        tags: vec![Tag {
                            key: ALIEN_STACK_TAG_KEY.to_string(),
                            value: live.prefix.clone(),
                        }],
                    }])
                    .build(),
            )
            .await
            .expect("stray security group")
            .group_id
            .expect("stray group id");

        // Polls without real waits, to reach the poll bound quickly.
        live.executor.delete().expect("delete transition");
        let error = live
            .drive(
                "delete with a stray SG",
                Duration::from_millis(200),
                |live| live.executor.status() == ResourceStatus::Deleted,
            )
            .await
            .expect_err("the VPC delete gives up while the stray group exists");
        assert_eq!(error.code, "RESOURCE_DELETE_BLOCKED", "{error}");
        assert!(error.message.contains(&stray), "names the blocker: {error}");
        println!("  delete blocked: {}", error.message);
        let controller = live.controller();
        assert_eq!(controller["state"], "deletingVpc");
        assert_eq!(controller["vpcId"].as_str(), Some(vpc_id.as_str()));
        assert_eq!(controller["publicSubnetIds"], serde_json::json!([]));
        assert!(controller["internetGatewayId"].is_null());
        let vpc_deletes = live.faulty.calls("delete_vpc");
        assert!(
            vpc_deletes
                .iter()
                .all(|call| call.outcome.contains("DependencyViolation")
                    || call.outcome.contains("dependencies")),
            "{vpc_deletes:?}"
        );
        println!("  delete_vpc polls before giving up: {}", vpc_deletes.len());

        // Remove the dependency and retry the failed delete, as a manual retry does (it
        // resets the poll count).
        live.real
            .delete_security_group(&stray)
            .await
            .expect("delete stray group");
        let mut retried = live
            .executor
            .internal_state::<AwsNetworkController>()
            .expect("network controller")
            .clone();
        retried.reset_stay_count();
        let mut live = rebuild(live, retried).await;
        let errors = live
            .drive_retrying("retried delete", Duration::from_secs(15), |live| {
                live.executor.status() == ResourceStatus::Deleted
            })
            .await;
        assert!(errors.is_empty(), "{errors:?}");
        assert!(live.controller()["vpcId"].is_null());
        assert_nothing_left(&live).await;
        live
    })
    .await;
}

/// 7. The `create_nat_gateway` response is lost and the create then fails for good, so the
/// NAT gateway ID is never recorded; delete finds it by its create token.
#[tokio::test]
#[ignore = "creates billed AWS resources; needs ALIEN_AWS_NETWORK_LIVE_TEST=1"]
async fn delete_finds_a_nat_gateway_whose_create_response_was_lost() {
    scenario("lost-nat", "10.237.0.0/16", |mut live| async move {
        live.faulty
            .inject("create_nat_gateway", 1, Fault::LoseResponse);
        live.faulty
            .inject("create_nat_gateway", 2, Fault::FailBeforeSend);
        let mut errors = live.create_until("creatingNatGateway").await;
        for _ in 0..2 {
            errors.push(
                live.drive("create", Duration::from_secs(15), |live| {
                    live.executor.status() == ResourceStatus::Running
                })
                .await
                .expect_err("NAT create fails")
                .to_string(),
            );
        }
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(live.controller()["natGatewayId"].is_null());
        let created = live.inventory().await;
        assert_eq!(created.nat_gateways.len(), 1, "{created:?}");

        live.delete_to_deleted().await;
        assert!(
            live.faulty.calls("delete_nat_gateway").len() == 1,
            "the recovered NAT gateway is deleted"
        );
        assert_nothing_left(&live).await;
        live
    })
    .await;
}

/// 8. A detached network interface that Lambda did not create (made here with
/// CreateNetworkInterface, so not AWS-managed) blocks the subnet delete. The delete never
/// deletes it and, at its poll bound, fails naming it; once it is gone the retried delete ends.
/// (A requester-managed Lambda interface cannot be made by hand; deleting those is covered by
/// the mock tests.)
#[tokio::test]
#[ignore = "creates billed AWS resources; needs ALIEN_AWS_NETWORK_LIVE_TEST=1"]
async fn delete_reports_but_never_deletes_a_detached_interface_lambda_did_not_create() {
    scenario("foreign-eni", "10.238.0.0/16", |mut live| async move {
        let errors = live.create_until("allocatingElasticIp").await;
        assert!(errors.is_empty(), "{errors:?}");
        let subnet = live.controller()["privateSubnetIds"][0]
            .as_str()
            .expect("a private subnet")
            .to_string();
        let foreign = live
            .real
            .create_network_interface(
                CreateNetworkInterfaceRequest::builder()
                    .subnet_id(subnet.clone())
                    .description("detached interface for the live network test".to_string())
                    .tag_specifications(vec![TagSpecification {
                        resource_type: "network-interface".to_string(),
                        tags: vec![Tag {
                            key: ALIEN_STACK_TAG_KEY.to_string(),
                            value: live.prefix.clone(),
                        }],
                    }])
                    .build(),
            )
            .await
            .expect("detached network interface")
            .network_interface
            .and_then(|interface| interface.network_interface_id)
            .expect("interface id");
        println!("  created detached interface {foreign} in {subnet}");

        // Delete until the subnet step has polled once on the interface.
        live.executor.delete().expect("delete transition");
        live.drive(
            "delete until the subnet waits",
            Duration::from_millis(200),
            |live| {
                live.controller()["state"] == "deletingSubnets"
                    && live.controller()["waitForDeleteDependenciesIterations"]
                        .as_u64()
                        .is_some_and(|polls| polls >= 1)
            },
        )
        .await
        .expect("the subnet step waits");

        // Resume that checkpoint two polls before its bound instead of waiting it out.
        let mut near_bound = live.controller();
        near_bound["waitForDeleteDependenciesIterations"] = serde_json::json!(88);
        let near_bound: AwsNetworkController =
            serde_json::from_value(near_bound).expect("controller");
        let mut live = rebuild(live, near_bound).await;
        let error = live
            .drive("delete at the bound", Duration::from_millis(200), |live| {
                live.executor.status() == ResourceStatus::Deleted
            })
            .await
            .expect_err("the subnet delete gives up");
        assert_eq!(error.code, "RESOURCE_DELETE_BLOCKED", "{error}");
        assert!(
            error.message.contains(&foreign),
            "names the interface: {error}"
        );
        println!("  delete blocked: {}", error.message);
        assert!(
            live.faulty.calls("delete_network_interface").is_empty(),
            "an interface Lambda did not create is never deleted"
        );

        live.real
            .delete_network_interface(&foreign)
            .await
            .expect("delete the interface");
        let mut retried = live
            .executor
            .internal_state::<AwsNetworkController>()
            .expect("network controller")
            .clone();
        retried.reset_stay_count();
        let mut live = rebuild(live, retried).await;
        let errors = live
            .drive_retrying("retried delete", Duration::from_secs(15), |live| {
                live.executor.status() == ResourceStatus::Deleted
            })
            .await;
        assert!(errors.is_empty(), "{errors:?}");
        assert_nothing_left(&live).await;
        live
    })
    .await;
}

/// The tags this controller puts on an object it creates under `token`.
fn create_attempt_tags(prefix: &str, resource_type: &str, token: &str) -> Vec<TagSpecification> {
    let mut tags: Vec<Tag> = standard_resource_tags(prefix, NETWORK_ID)
        .into_iter()
        .map(|(key, value)| Tag { key, value })
        .collect();
    tags.push(Tag {
        key: "CreateAttempt".to_string(),
        value: token.to_string(),
    });
    vec![TagSpecification {
        resource_type: resource_type.to_string(),
        tags,
    }]
}

/// 9. A create repeated after a read missed the first object leaves two objects under one
/// create token, with only one ID recorded. Here a second VPC and a second internet gateway
/// are made with the controller's own tokens; delete finds both by token and removes them.
#[tokio::test]
#[ignore = "creates billed AWS resources; needs ALIEN_AWS_NETWORK_LIVE_TEST=1"]
async fn delete_removes_duplicates_created_under_the_same_token() {
    scenario("dup-token", "10.239.0.0/16", |mut live| async move {
        let errors = live.create_until("creatingSubnets").await;
        assert!(errors.is_empty(), "{errors:?}");
        let controller = live.controller();
        let vpc_token = controller["vpcCreateToken"]
            .as_str()
            .expect("VPC token")
            .to_string();
        let igw_token = controller["internetGatewayCreateToken"]
            .as_str()
            .expect("IGW token")
            .to_string();
        let cidr = controller["cidrBlock"].as_str().expect("CIDR").to_string();

        let duplicate_vpc = live
            .real
            .create_vpc(
                CreateVpcRequest::builder()
                    .cidr_block(cidr)
                    .tag_specifications(create_attempt_tags(&live.prefix, "vpc", &vpc_token))
                    .build(),
            )
            .await
            .expect("duplicate VPC")
            .vpc
            .and_then(|vpc| vpc.vpc_id)
            .expect("VPC id");
        let duplicate_igw = live
            .real
            .create_internet_gateway(
                CreateInternetGatewayRequest::builder()
                    .tag_specifications(create_attempt_tags(
                        &live.prefix,
                        "internet-gateway",
                        &igw_token,
                    ))
                    .build(),
            )
            .await
            .expect("duplicate IGW")
            .internet_gateway
            .and_then(|igw| igw.internet_gateway_id)
            .expect("IGW id");
        println!("  duplicates under the controller's tokens: {duplicate_vpc}, {duplicate_igw}");
        let before = live.inventory().await;
        assert_eq!(before.vpcs.len(), 2, "{before:?}");
        assert_eq!(before.internet_gateways.len(), 2, "{before:?}");

        live.delete_to_deleted().await;
        let vpc_deletes: Vec<_> = live
            .faulty
            .calls("delete_vpc")
            .into_iter()
            .filter(|call| call.outcome == "ok")
            .collect();
        assert_eq!(vpc_deletes.len(), 2, "both VPCs deleted: {vpc_deletes:?}");
        let igw_deletes: Vec<_> = live
            .faulty
            .calls("delete_internet_gateway")
            .into_iter()
            .filter(|call| call.outcome == "ok")
            .collect();
        assert_eq!(igw_deletes.len(), 2, "both IGWs deleted: {igw_deletes:?}");
        let controller = live.controller();
        assert_eq!(controller["extraVpcIds"], serde_json::json!([]));
        assert_eq!(controller["extraInternetGatewayIds"], serde_json::json!([]));
        assert_nothing_left(&live).await;
        live
    })
    .await;
}

/// A new executor for the same scenario and EC2 wrapper, continuing from `controller`.
async fn rebuild(live: Live, controller: AwsNetworkController) -> Live {
    let config = aws_config();
    let ec2: Arc<dyn Ec2Api> = live.faulty.clone();
    let quotas = DefaultPlatformServiceProvider::default()
        .get_aws_service_quotas_client(&config)
        .await
        .expect("Service Quotas client");
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_ec2_client()
        .returning(move |_| Ok(ec2.clone()));
    provider
        .expect_get_aws_service_quotas_client()
        .returning(move |_| Ok(quotas.clone()));
    let network = Network::new(NETWORK_ID.to_string())
        .settings(NetworkSettings::Create {
            cidr: live.controller()["cidrBlock"].as_str().map(str::to_string),
            availability_zones: 2,
        })
        .build();
    let executor = SingleControllerExecutor::builder()
        .resource(network)
        .controller(controller)
        .platform(Platform::Aws)
        .client_config(ClientConfig::Aws(Box::new(config)))
        .service_provider(Arc::new(provider))
        .resource_prefix(live.prefix.clone())
        .build()
        .await
        .expect("executor builds");
    Live { executor, ..live }
}

/// AWS actually denies SG deletion through a restricted session; other children still
/// disappear. Restore the full client and resume a serialized parent checkpoint.
/// The restricted credentials must permit the same reads/deletes except for an explicit
/// deny on ec2:DeleteSecurityGroup. Use an STS session policy, never change shared IAM.
#[tokio::test]
#[ignore = "creates billed AWS resources; needs live and restricted-session credentials"]
async fn delete_resumes_retained_children_after_real_permission_restoration() {
    let mut restricted = aws_config();
    restricted.credentials = AwsCredentials::AccessKeys {
        access_key_id: env("ALIEN_AWS_NETWORK_DENIED_ACCESS_KEY_ID"),
        secret_access_key: env("ALIEN_AWS_NETWORK_DENIED_SECRET_ACCESS_KEY"),
        session_token: Some(env("ALIEN_AWS_NETWORK_DENIED_SESSION_TOKEN")),
    };
    let denied = DefaultPlatformServiceProvider::default()
        .get_aws_ec2_client(&restricted)
        .await
        .expect("restricted real EC2 client");
    scenario("restore-perm", "10.240.0.0/16", |mut live| async move {
        assert!(live.create_to_running().await.is_empty());
        let created = live.inventory().await;
        assert_full_network(&created);
        let group = created.security_groups[0].clone();
        let vpc = created.vpcs[0].clone();
        live.faulty.set_client(denied);
        live.executor.delete().expect("delete transition");
        live.drive(
            "delete while SG permission is denied",
            Duration::from_secs(15),
            |live| live.controller()["waitForRetainedDeleteIterations"].as_u64() == Some(1),
        )
        .await
        .expect("retained children cause another cleanup pass");
        assert_ne!(live.executor.status(), ResourceStatus::Deleted);
        let remaining = live.inventory().await;
        assert_eq!(remaining.vpcs, vec![vpc.clone()]);
        assert_eq!(remaining.security_groups, vec![group.clone()]);
        assert!(
            remaining.subnets.is_empty(),
            "independent subnet cleanup: {remaining:?}"
        );
        assert!(
            live.faulty.calls("delete_vpc").is_empty(),
            "parent stays untouched"
        );
        let denied_calls = live.faulty.calls("delete_security_group");
        assert!(
            denied_calls
                .iter()
                .any(|call| call.outcome.contains("REMOTE_ACCESS_DENIED")),
            "AWS must actually deny the request: {denied_calls:?}"
        );
        let mut checkpoint = live.controller();
        assert_eq!(checkpoint["securityGroupId"].as_str(), Some(group.as_str()));
        assert_eq!(checkpoint["vpcId"].as_str(), Some(vpc.as_str()));
        // Also cover a parent checkpoint saved by an older controller version.
        checkpoint["state"] = serde_json::json!("deletingVpc");
        let checkpoint: AwsNetworkController =
            serde_json::from_value(checkpoint).expect("reload checkpoint");
        live.faulty.set_client(live.real.clone());
        let mut resumed = rebuild(live, checkpoint).await;
        assert!(resumed
            .drive_retrying(
                "resume after restoring permission",
                Duration::from_secs(15),
                |live| { live.executor.status() == ResourceStatus::Deleted }
            )
            .await
            .is_empty());
        assert!(resumed.faulty.calls("delete_security_group").len() >= 2);
        assert_nothing_left(&resumed).await;
        resumed
    })
    .await;
}
