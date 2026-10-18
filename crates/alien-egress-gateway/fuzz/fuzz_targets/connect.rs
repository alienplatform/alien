#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = alien_egress_gateway::protocol::request_target(data);
});
