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
    server::{self, Gateway},
    store::PolicyStore,
};
use alien_error::{AlienError, Context, IntoAlienError};
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
    let encoded = match env::var("ALIEN_EGRESS_GATEWAY_CONFIG") {
        Ok(encoded) => encoded,
        Err(_) => {
            let path = env::args().nth(1).ok_or_else(|| {
                AlienError::new(failed("provide ALIEN_EGRESS_GATEWAY_CONFIG or config path"))
            })?;
            tokio::fs::read_to_string(path)
                .await
                .into_alien_error()
                .context(failed("read config"))?
        }
    };
    let config: Config = serde_json::from_str(&encoded)
        .into_alien_error()
        .context(failed("parse config"))?;
    let keys = config.verification_keys()?;
    credentials::validate_rules(&config.credentials)?;
    let bindings = Bindings::from_env().context(failed("load deployment bindings"))?;
    let kv = bindings
        .kv(&config.generation_store.binding)
        .await
        .context(failed("load policy binding"))?;
    let proxy = TcpListener::bind(config.listen)
        .await
        .into_alien_error()
        .context(failed("bind proxy"))?;
    let admin_listener = TcpListener::bind(config.admin)
        .await
        .into_alien_error()
        .context(failed("bind admin"))?;
    let credentials = Credentials::new(kv.clone(), bindings, &config.upstream_ca_certificates)?;
    let gateway = Arc::new(Gateway {
        config,
        keys,
        policies: PolicyStore::new(kv.clone()),
        credentials,
        accepted: AtomicU64::new(0),
        denied: AtomicU64::new(0),
    });
    let admin = Arc::new(Admin {
        gateway: gateway.clone(),
    });
    tokio::select! {
        result = server::serve(proxy, gateway) => result,
        result = axum::serve(admin_listener, admin::router(admin)) => result.into_alien_error().context(failed("serve admin")),
        result = tokio::signal::ctrl_c() => result.into_alien_error().context(failed("shutdown signal")),
    }
}
