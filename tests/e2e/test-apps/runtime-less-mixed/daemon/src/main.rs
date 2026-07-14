//! Rust Daemon half of the mixed runtime-less E2E fixture.

use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const RESOURCE: &str = "rust-daemon";
const OWN_KEYS: [&str; 4] = ["rust:0", "rust:1", "rust:2", "rust:3"];
const PEER_KEYS: [&str; 4] = [
    "typescript:0",
    "typescript:1",
    "typescript:2",
    "typescript:3",
];

async fn count_existing(
    kv: &dyn alien_bindings::Kv,
    keys: &[&str],
) -> alien_bindings::Result<usize> {
    let mut count = 0;
    for key in keys {
        if kv.get(key).await?.is_some() {
            count += 1;
        }
    }
    Ok(count)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    let bindings = alien_bindings::Bindings::from_env()?;
    let kv = bindings.kv("index").await?;
    for key in OWN_KEYS {
        kv.put(key, format!("seeded by {RESOURCE}").into_bytes(), None)
            .await?;
    }

    let mut receiver = alien_commands::receiver::Receiver::from_env()?;
    receiver.handle("status", move |_ctx| {
        let kv = kv.clone();
        async move {
            Ok(serde_json::json!({
                "resource": RESOURCE,
                "role": "daemon",
                "language": "rust",
                "model": "pull",
                "ownDocuments": count_existing(kv.as_ref(), &OWN_KEYS).await?,
                "peerDocuments": count_existing(kv.as_ref(), &PEER_KEYS).await?,
            }))
        }
    });

    info!(resource = RESOURCE, "Rust Daemon leasing commands");
    receiver.run().await?;
    Ok(())
}
