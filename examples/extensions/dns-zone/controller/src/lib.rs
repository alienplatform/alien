//! The zone's controllers for AWS, Google Cloud and Azure. `alien extensions build` builds this
//! library for wasm32-wasip1 into the extension's controller module, which Alien runs to create,
//! update and delete each zone. `cargo test` builds it natively.

use acme_types::DnsZone;
use alien_extension_sdk::controller_prelude::*;

pub mod aws;
pub mod azure;
pub mod gcp;

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
}

alien_extension_sdk::export_controllers!(register);
