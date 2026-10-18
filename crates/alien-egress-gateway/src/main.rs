use std::{
    env,
    sync::{atomic::AtomicU64, Arc},
};

use alien_bindings::Bindings;
use alien_egress_gateway::{
    admin::{self, Admin},
    config::Config,
    credentials::{self, Credentials},
    error::{ErrorData, Result},
    generation::GenerationFence,
    server::{self, Gateway},
};
use alien_error::{AlienError, Context, IntoAlienError};
use tokio::net::TcpListener;

fn failed(operation: &str) -> ErrorData {
    ErrorData::Unavailable {
        operation: operation.into(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let path = env::args()
        .nth(1)
        .ok_or_else(|| AlienError::new(failed("provide gateway config path")))?;
    let config: Config = serde_json::from_slice(
        &tokio::fs::read(path)
            .await
            .into_alien_error()
            .context(failed("read config"))?,
    )
    .into_alien_error()
    .context(failed("parse config"))?;
    let keys = config.verification_keys()?;
    credentials::validate_rules(&config.credentials)?;
    let token = env::var("ALIEN_EGRESS_ADMIN_TOKEN")
        .into_alien_error()
        .context(failed("read ALIEN_EGRESS_ADMIN_TOKEN"))?;
    let bindings = Bindings::from_env().context(failed("load deployment bindings"))?;
    let kv = bindings
        .kv(&config.generation_binding)
        .await
        .context(failed("load generation binding"))?;
    let proxy = TcpListener::bind(config.listen)
        .await
        .into_alien_error()
        .context(failed("bind proxy"))?;
    let admin_listener = TcpListener::bind(config.admin_listen)
        .await
        .into_alien_error()
        .context(failed("bind admin"))?;
    let credentials = Credentials::new(kv.clone(), bindings, &config.upstream_ca_certificates)?;
    let gateway = Arc::new(Gateway {
        config,
        keys,
        generations: GenerationFence::new(kv.clone()),
        credentials,
        accepted: AtomicU64::new(0),
        denied: AtomicU64::new(0),
    });
    let admin = Arc::new(Admin::new(gateway.clone(), &token)?);
    tokio::select! {
        result = server::serve(proxy, gateway) => result,
        result = axum::serve(admin_listener, admin::router(admin)) => result.into_alien_error().context(failed("serve admin")),
        result = tokio::signal::ctrl_c() => result.into_alien_error().context(failed("shutdown signal")),
    }
}
