use acme_types::DnsZone;
use alien_extension_sdk::controller_prelude::*;

pub mod aws;
pub mod azure;
pub mod gcp;
pub mod probe;

/// Registers the zone's controller for each cloud.
pub fn register(controllers: &mut Controllers) {
    controllers.register(
        DnsZone::RESOURCE_TYPE,
        Platform::Aws,
        aws::AwsDnsZoneController::default(),
    );
    controllers.register(
        DnsZone::RESOURCE_TYPE,
        Platform::Gcp,
        gcp::GcpDnsZoneController::default(),
    );
    controllers.register(
        DnsZone::RESOURCE_TYPE,
        Platform::Azure,
        azure::AzureDnsZoneController::default(),
    );
    controllers.register(
        probe::Probe::RESOURCE_TYPE,
        Platform::Test,
        probe::ProbeController::default(),
    );
}

alien_extension_sdk::export_controllers!(register);
