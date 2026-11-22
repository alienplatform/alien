use std::{
    env,
    sync::{atomic::AtomicU64, Arc},
    time::Duration,
};

use alien_bindings::Bindings;
use alien_egress_gateway::{
    admin,
    error::{ErrorData, Result},
    logging, relay,
    runtime::{self, ConfigSource, LiveGateway},
    server,
};
use alien_error::{Context, IntoAlienError};
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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
    // `alien-egress-gateway --self-check`: the release build runs it. A gateway built without the
    // cloud providers boots, cannot read its configuration vault and restarts forever on a VM,
    // so a build that lacks them must fail where it is built.
    if env::args().nth(1).as_deref() == Some("--self-check") {
        let cloud = cfg!(feature = "cloud");
        println!(
            "cloud providers: {}",
            if cloud { "compiled in" } else { "MISSING" }
        );
        std::process::exit(if cloud { 0 } else { 1 });
    }
    // `--relay <listen> <target>`: the per-session preview forwarder of `alien dev`.
    if env::args().nth(1).as_deref() == Some("--relay") {
        let (Some(listen), Some(target)) = (env::args().nth(2), env::args().nth(3)) else {
            return Err(alien_error::AlienError::new(failed(
                "--relay needs <listen> <target>",
            )));
        };
        let listen = listen
            .parse()
            .into_alien_error()
            .context(failed("parse the relay listen address"))?;
        return relay::run(listen, target)
            .await
            .into_alien_error()
            .context(failed("relay"));
    }
    tracing_subscriber::registry()
        .with(logging::json_layer(std::io::stdout))
        .with(EnvFilter::from_default_env())
        .init();
    let bindings = Arc::new(Bindings::from_env().context(failed("load deployment bindings"))?);
    // An inline configuration or a command-line file is static. A refreshing gateway reads a
    // file someone else keeps current (`ALIEN_EGRESS_GATEWAY_CONFIG_FILE`, a Kubernetes Secret
    // the kubelet mounts), or else its vault binding.
    let (encoded, refresh) = match env::var("ALIEN_EGRESS_GATEWAY_CONFIG") {
        Ok(encoded) => (encoded, None),
        Err(_) => match env::args().nth(1) {
            Some(path) => (
                tokio::fs::read_to_string(path)
                    .await
                    .into_alien_error()
                    .context(failed("read config override"))?,
                None,
            ),
            None => {
                let source = env::var_os("ALIEN_EGRESS_GATEWAY_CONFIG_FILE")
                    .map_or(ConfigSource::Vault, |path| ConfigSource::File(path.into()));
                let encoded = tokio::time::timeout(
                    Duration::from_secs(10),
                    runtime::read_config(&bindings, &source),
                )
                .await
                .into_alien_error()
                .context(failed("boot config timeout"))??;
                (encoded, Some(source))
            }
        },
    };
    let gateway = runtime::build(
        &encoded,
        bindings.clone(),
        Arc::new(AtomicU64::new(0)),
        Arc::new(AtomicU64::new(0)),
    )
    .await?;
    let proxy_address = gateway.config.listen;
    let admin_listener = TcpListener::bind(gateway.config.admin)
        .await
        .into_alien_error()
        .context(failed("bind admin"))?;
    let refreshing = refresh.is_some();
    let gateway = Arc::new(match refresh {
        Some(source) => LiveGateway::refreshing(gateway, source),
        None => LiveGateway::static_config(gateway),
    });
    let refresh_config = async {
        if !refreshing {
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
    // The proxy port opens only once this replica has caught up its policies. A replica that
    // listens before that accepts every connection and refuses it, and an agent fails over only
    // when the connect itself fails: closed means "try the other replica", open-and-refusing
    // would pin idle sandboxes to a replica that denies them all.
    let proxy = {
        let live = gateway.clone();
        let mut closing = receiver.clone();
        async move {
            while !live.snapshot().is_some_and(|g| g.policies.ready()) {
                if *closing.borrow() {
                    return Ok(());
                }
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                    _ = closing.changed() => {}
                }
            }
            let listener = TcpListener::bind(proxy_address)
                .await
                .into_alien_error()
                .context(failed("bind proxy"))?;
            server::serve(listener, live, receiver).await
        }
    };
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
