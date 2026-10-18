#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = alien_egress_gateway::protocol::client_hello_name(data);
});
