use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use alien_operations_sdk::{PluginInvocation, PluginResult};
use axum::{
    extract::State,
    routing::{get, put},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::net::TcpListener;

#[derive(Default)]
struct Application {
    throttle: AtomicU32,
    requests: AtomicUsize,
}

async fn health(State(app): State<Arc<Application>>) -> Json<Value> {
    app.requests.fetch_add(1, Ordering::SeqCst);
    Json(json!({ "status": "healthy", "maxExportsPerMinute": app.throttle.load(Ordering::SeqCst) }))
}

async fn throttle(State(app): State<Arc<Application>>, Json(params): Json<Value>) -> Json<Value> {
    app.requests.fetch_add(1, Ordering::SeqCst);
    let limit = params["maxExportsPerMinute"]
        .as_u64()
        .expect("typed throttle");
    app.throttle
        .store(u32::try_from(limit).expect("u32 limit"), Ordering::SeqCst);
    Json(params)
}

async fn application() -> (String, Arc<Application>, tokio::task::JoinHandle<()>) {
    let app = Arc::new(Application::default());
    app.throttle.store(100, Ordering::SeqCst);
    let router = Router::new()
        .route("/health", get(health))
        .route("/admin/export-throttle", put(throttle))
        .with_state(Arc::clone(&app));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind application");
    let url = format!(
        "http://{}",
        listener.local_addr().expect("application address")
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("serve application");
    });
    (url, app, server)
}

async fn invoke(url: &str, invocation: PluginInvocation) -> PluginResult {
    let url = url.to_string();
    tokio::task::spawn_blocking(move || {
        let mut child = Command::new(env!("CARGO_BIN_EXE_custom-ops"))
            .env("CUSTOM_OPS_BASE_URL", url)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start plugin");
        child
            .stdin
            .take()
            .expect("plugin stdin")
            .write_all(&serde_json::to_vec(&invocation).expect("encode invocation"))
            .expect("send invocation");
        let output = child.wait_with_output().expect("wait for plugin");
        assert!(
            output.status.success(),
            "plugin failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("one protocol result on stdout")
    })
    .await
    .expect("join plugin")
}

fn success(result: PluginResult) -> Value {
    let PluginResult::Success { response } = result else {
        panic!("operation failed: {result:?}");
    };
    serde_json::from_slice(&response.decode_inline().expect("inline response"))
        .expect("JSON response")
}

#[tokio::test]
async fn plugin_reads_health_and_changes_the_application_throttle() {
    let (url, app, server) = application().await;
    let doctor = success(invoke(&url, PluginInvocation::inline_json("doctor", b"{}")).await);
    assert_eq!(
        doctor,
        json!({ "status": "healthy", "maxExportsPerMinute": 100 })
    );
    let changed = success(
        invoke(
            &url,
            PluginInvocation::inline_json(
                "throttle-export-automation",
                br#"{"maxExportsPerMinute":12}"#,
            ),
        )
        .await,
    );
    assert_eq!(changed, json!({ "maxExportsPerMinute": 12 }));
    assert_eq!(app.throttle.load(Ordering::SeqCst), 12);
    let doctor = success(invoke(&url, PluginInvocation::inline_json("doctor", b"{}")).await);
    assert_eq!(doctor["maxExportsPerMinute"], 12);
    assert_eq!(app.requests.load(Ordering::SeqCst), 3);
    server.abort();
}

#[tokio::test]
async fn invalid_params_unknown_names_and_protocol_versions_never_contact_the_application() {
    let (url, app, server) = application().await;
    for params in [
        b"{}".as_slice(),
        br#"{"maxExportsPerMinute":0}"#,
        br#"{"maxExportsPerMinute":-1}"#,
        br#"{"maxExportsPerMinute":1,"extra":true}"#,
    ] {
        let result = invoke(
            &url,
            PluginInvocation::inline_json("throttle-export-automation", params),
        )
        .await;
        assert!(
            matches!(result, PluginResult::Error { ref code, .. } if code == "INVALID_PARAMS"),
            "{result:?}"
        );
    }
    let result = invoke(&url, PluginInvocation::inline_json("missing", b"{}")).await;
    assert!(
        matches!(result, PluginResult::Error { ref code, .. } if code == "OPERATION_UNKNOWN"),
        "{result:?}"
    );
    let mut unsupported = PluginInvocation::inline_json("doctor", b"{}");
    unsupported.protocol_version = u32::MAX;
    let result = invoke(&url, unsupported).await;
    assert!(
        matches!(result, PluginResult::Error { ref code, .. } if code == "PLUGIN_PROTOCOL_VERSION_UNSUPPORTED"),
        "{result:?}"
    );
    assert_eq!(app.requests.load(Ordering::SeqCst), 0);
    assert_eq!(app.throttle.load(Ordering::SeqCst), 100);
    server.abort();
}

#[test]
fn published_metadata_matches_the_runtime_registry() {
    let published: Value =
        serde_json::from_str(include_str!("../metadata.json")).expect("published metadata");
    let generated =
        serde_json::to_value(custom_ops::plugin_manifest().expect("generated manifest"))
            .expect("serialize manifest");
    assert_eq!(published, generated);
}

#[test]
fn published_metadata_with_decimal_schema_bounds_parses_as_a_manifest() {
    // Generated JSON Schemas write integer bounds as decimals (`"minimum": 1.0`)
    // inside an untagged schema type. A dependency that enables serde_json's
    // `arbitrary_precision` feature breaks this decode for every crate in the
    // build, so `alien operations` commands would reject the manifest.
    let bytes = include_bytes!("../metadata.json");
    assert!(
        String::from_utf8_lossy(bytes).contains("\"minimum\": 1.0"),
        "the fixture must keep a decimal schema bound"
    );
    let manifest = alien_operations_sdk::CanonicalPluginManifest::parse_and_validate(bytes)
        .expect("published metadata must parse as a plugin manifest");
    assert_eq!(
        serde_json::to_value(&manifest).expect("serialize manifest"),
        serde_json::from_slice::<Value>(bytes).expect("published metadata"),
    );
}
