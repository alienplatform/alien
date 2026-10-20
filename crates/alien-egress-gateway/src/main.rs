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
    let gateway = Arc::new(if refresh {
        LiveGateway::new(gateway)
    } else {
        LiveGateway::static_config(gateway)
    });
    let refresh_config = async {
        if !refresh {
            std::future::pending::<()>().await;
        }
        let mut interval = tokio::time::interval(refresh_interval());
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            if let Err(error) = gateway.refresh(&bindings).await {
                // Do not log vault content or nested errors that might contain its value.
                tracing::error!(code = %error.code, "gateway config refresh failed; last-good window applies");
            }
        }
    };
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let proxy = server::serve(proxy, gateway.clone(), receiver);
    let admin_shutdown = shutdown.subscribe();
    let admin = axum::serve(admin_listener, admin::router(gateway.clone())).with_graceful_shutdown(
        async move {
            let mut receiver = admin_shutdown;
            let _ = receiver.changed().await;
        },
    );
    tokio::pin!(proxy);
    let admin = std::future::IntoFuture::into_future(admin);
    tokio::pin!(admin);
    tokio::select! {
        result = &mut proxy => return result,
        result = &mut admin => return result.into_alien_error().context(failed("serve admin")),
        result = refresh_config => return result,
        result = alien_egress_gateway::replication::run(gateway.clone()) => return result,
        result = shutdown_signal() => result?,
    }
    gateway.stop();
    shutdown.send_replace(true);
    // One deadline bounds both proxy and admin drain, including hung dependencies.
    let _ = tokio::time::timeout(Duration::from_secs(16), async {
        let _ = tokio::join!(proxy, admin);
    })
    .await;
    Ok(())
}

/// How often the configuration is re-read: 60 seconds, or `ALIEN_EGRESS_GATEWAY_REFRESH_SECONDS`
/// (at least one) for tests that exercise configuration changes.
fn refresh_interval() -> Duration {
    let seconds = env::var("ALIEN_EGRESS_GATEWAY_REFRESH_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map_or(60, |value| value.max(1));
    Duration::from_secs(seconds)
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .into_alien_error()
            .context(failed("install SIGTERM handler"))?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.into_alien_error().context(failed("shutdown signal")),
            _ = term.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c()
            .await
            .into_alien_error()
            .context(failed("shutdown signal"))
    }
}
