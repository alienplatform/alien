//! Serves `alien_infra::step_protocol::test_controller` over stdin and stdout, for the step
//! protocol's tests. `MOCK_CLOUD_DIR` is its cloud; `STEP_LOG`, if set, gets each step.

use alien_infra::step_protocol::test_controller::ControllerHost;

fn main() -> std::io::Result<()> {
    let cloud_dir = std::env::var("MOCK_CLOUD_DIR").expect("MOCK_CLOUD_DIR is set");
    let log = std::env::var("STEP_LOG").ok().map(Into::into);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    ControllerHost::new(cloud_dir, log).serve_stdio(&runtime)
}
