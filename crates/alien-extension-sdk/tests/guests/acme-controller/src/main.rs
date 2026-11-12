use acme_types::DnsZone;
use alien_extension_sdk::controller_prelude::*;

mod aws;
mod azure;
mod gcp;
mod local;

fn main() -> std::process::ExitCode {
    alien_extension_sdk::run_controllers(|controllers| {
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
        // alien dev: records the zone and returns a placeholder id, with no cloud call.
        controllers.register(
            DnsZone::RESOURCE_TYPE,
            Platform::Local,
            local::LocalDnsZoneController::default(),
        );
    })
}
