//! Rust source Container for the local pull E2E.
//!
//! Proves the runtime-less Container contract for Rust apps, mirroring what
//! `command-routing-ts` proves for TypeScript Daemons:
//!
//! - the compiled binary is the image entrypoint (no runtime wrapper);
//! - bindings are direct and in-process (`Bindings::from_env`);
//! - commands arrive through the app-owned pull receiver
//!   (`alien_commands::Receiver`), target-scoped to this container.
//!
//! On startup the app seeds a fixed document set into its `index` KV binding.
//! The `status` handler counts the seeded documents back out of KV, so a
//! successful command response proves an in-process KV round-trip, not just
//! command delivery.

use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Seeded into the `index` KV on startup; `status` counts them back.
const DOCUMENTS: [(&str, &str); 4] = [
    ("doc:workers", "Workers handle platform events"),
    ("doc:containers", "Containers are plain service processes"),
    ("doc:daemons", "Daemons are resident processes"),
    ("doc:commands", "Commands are target-scoped"),
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    info!("container-rust-indexer starting (runtime-less)");

    // Direct in-process bindings: no runtime gRPC, no ALIEN_BINDINGS_GRPC_ADDRESS.
    let bindings = alien_bindings::Bindings::from_env()?;
    let kv = bindings.kv("index").await?;

    for (key, value) in DOCUMENTS {
        kv.put(key, value.as_bytes().to_vec(), None).await?;
    }
    info!(count = DOCUMENTS.len(), "Seeded index documents");

    // App-owned pull receiver: leases only this container's commands.
    let mut receiver = alien_commands::receiver::Receiver::from_env()?;
    receiver.command("status", move |_: serde_json::Value, _ctx| {
        let kv = kv.clone();
        async move {
            let mut documents = 0;
            for (key, _) in DOCUMENTS {
                if kv.get(key).await?.is_some() {
                    documents += 1;
                }
            }
            Ok(serde_json::json!({
                "resource": "indexer",
                "role": "container",
                "model": "pull",
                "documents": documents,
            }))
        }
    });

    receiver.run().await?;
    Ok(())
}
