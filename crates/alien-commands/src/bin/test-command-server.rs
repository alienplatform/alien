//! Standalone test command server for the TypeScript integration suite.
//!
//! Spins up a [`TestCommandServer`] in pull mode (which auto-registers a
//! `test-daemon` target), prints `READY {base_url}` to stdout, and then runs
//! until the parent closes our stdin (graceful) or sends `Ctrl-C`. This is the
//! external process the TS `integration.real-server.test.ts` suite drives to
//! prove the sender/receiver twins over the *real* Rust command wire.
//!
//! Gated behind the `test-utils` feature via `required-features` in
//! `Cargo.toml`, so it is never part of a normal build.

use std::io::Write;

use alien_commands::test_utils::TestCommandServer;
use alien_commands::CommandTargetType;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() {
    let server = TestCommandServer::builder().with_pull_mode().build().await;

    // Optionally register an extra target:
    //   test-command-server <resource_id> [container|daemon|worker]
    // (env fallbacks let callers configure without argv). The default
    // `test-daemon` target is already auto-registered by pull mode.
    let mut args = std::env::args().skip(1);
    if let Some(resource_id) = args
        .next()
        .or_else(|| std::env::var("TEST_TARGET_RESOURCE_ID").ok())
    {
        let resource_type = args
            .next()
            .or_else(|| std::env::var("TEST_TARGET_RESOURCE_TYPE").ok());
        let resource_type = match resource_type.as_deref() {
            Some("container") => CommandTargetType::Container,
            Some("worker") => CommandTargetType::Worker,
            _ => CommandTargetType::Daemon,
        };
        server
            .registry
            .register_target(resource_id, resource_type)
            .await
            .expect("register extra target");
    }

    println!("READY {}", server.base_url());
    std::io::stdout().flush().expect("flush READY line");

    // Run until the parent closes stdin (graceful shutdown, drops the server and
    // its temp dir) or interrupts us.
    let mut stdin = tokio::io::stdin();
    let mut buf = [0u8; 64];
    loop {
        tokio::select! {
            read = stdin.read(&mut buf) => {
                if matches!(read, Ok(0) | Err(_)) {
                    break;
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
}
