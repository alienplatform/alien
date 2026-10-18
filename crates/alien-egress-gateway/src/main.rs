use std::{
    env,
    sync::{atomic::AtomicU64, Arc},
    time::Duration,
};

use alien_bindings::Bindings;
use alien_egress_gateway::{
    admin,
    error::{ErrorData, Result},
    runtime::{self, LiveGateway},
    server,
};
use alien_error::{Context, IntoAlienError};
use tokio::net::TcpListener;

fn failed(operation: &str) -> ErrorData {
    ErrorData::Unavailable {
        operation: operation.into(),
    }
}

#[tokio::main]
#[expect(
    clippy::result_large_err,
    reason = "Alien errors retain structured metadata and causal context"
)]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let bindings = Arc::new(Bindings::from_env().context(failed("load deployment bindings"))?);
    let (encoded, refresh) = match env::var("ALIEN_EGRESS_GATEWAY_CONFIG") {
        Ok(encoded) => (encoded, false),
        Err(_) => match env::args().nth(1) {
            Some(path) => (
                tokio::fs::read_to_string(path)
                    .await
                    .into_alien_error()
                    .context(failed("read config override"))?,
                false,
            ),
            None => (
                tokio::time::timeout(
                    Duration::from_secs(10),
                    runtime::read_vault_config(&bindings),
                )
                .await
                .into_alien_error()
                .context(failed("boot config timeout"))??,
                true,
            ),
        },
    };
    let gateway = runtime::build(
        &encoded,
        bindings.clone(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    )
    .await?;
    let proxy = TcpListener::bind(gateway.config.listen)
        .await
        .into_alien_error()
        .context(failed("bind proxy"))?;
    let admin_listener = TcpListener::bind(gateway.config.admin)
        .await
        .into_alien_error()
        .context(failed("bind admin"))?;
    let gateway = Arc::new(LiveGateway::new(gateway));
    let refresh_config = async {
        if !refresh {
            std::future::pending::<()>().await;
        }
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = gateway.refresh(&bindings).await {
                // Do not log vault content or nested errors that might contain its value.
                tracing::error!(code = %error.code, "gateway config unavailable; new requests denied");
            }
        }
    };
    tokio::select! {
        result = server::serve(proxy, gateway.clone()) => result,
        result = axum::serve(admin_listener, admin::router(gateway.clone())) => result.into_alien_error().context(failed("serve admin")),
        result = refresh_config => result,
        result = tokio::signal::ctrl_c() => result.into_alien_error().context(failed("shutdown signal")),
    }
}
