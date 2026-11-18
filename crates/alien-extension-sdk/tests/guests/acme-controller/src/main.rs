use acme_types::DnsZone;
use alien_extension_sdk::local_controller_prelude::*;

mod local;

fn main() -> std::process::ExitCode {
    alien_extension_sdk::run_controllers(|controllers| {
        // alien dev: records the zone and returns a placeholder id, with no cloud call.
        controllers.register(
            DnsZone::RESOURCE_TYPE,
            Platform::Local,
            local::LocalDnsZoneController::default(),
        );
    })
}
