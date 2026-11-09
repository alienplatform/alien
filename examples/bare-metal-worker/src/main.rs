//! A job worker for Linux servers. It takes jobs over HTTP, runs them in-process, and on
//! SIGTERM or SIGINT stops taking jobs, lets the running ones finish, and exits.

mod api;
mod worker;

use std::env;
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use tokio::net::TcpListener;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::oneshot;
use tracing::info;

use crate::worker::{Config, Worker, VERSION};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(false)
        .without_time()
        .with_ansi(false)
        .init();

    let port: u16 = from_env("PORT", 8080)?;
    let config = Config {
        concurrency: from_env("WORKER_CONCURRENCY", 4)?,
        queue_capacity: from_env("QUEUE_CAPACITY", 100)?,
    };

    // Handlers are installed before the port opens, so a signal can never arrive while
    // the process still dies by default.
    let mut sigterm = signal(SignalKind::terminate()).context("install the SIGTERM handler")?;
    let mut sigint = signal(SignalKind::interrupt()).context("install the SIGINT handler")?;

    let worker = Worker::start(config).map_err(|message| anyhow!(message))?;
    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("listen on port {port}"))?;
    info!(
        "job-worker {VERSION} listening on {}",
        listener.local_addr().context("read the listen address")?
    );

    let (drained, wait_for_drain) = oneshot::channel();
    tokio::spawn({
        let worker = worker.clone();
        async move {
            let name = tokio::select! {
                _ = sigterm.recv() => "SIGTERM",
                _ = sigint.recv() => "SIGINT",
            };
            info!("received {name}");
            worker.drain().await;
            info!("drained, exiting");
            let _ = drained.send(());
        }
    });

    axum::serve(listener, api::router(worker))
        .with_graceful_shutdown(async {
            let _ = wait_for_drain.await;
        })
        .await
        .context("serve HTTP")
}

fn from_env<T: FromStr>(name: &str, default: T) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(name) {
        Ok(value) => value
            .parse()
            .with_context(|| format!("{name} is not valid: {value:?}")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("read {name}")),
    }
}
