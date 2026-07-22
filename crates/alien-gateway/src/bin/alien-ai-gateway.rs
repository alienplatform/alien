//! Container AI-gateway launcher. A thin bootstrap that hosts the embedded gateway
//! for a workload whose image is not built through alien-runtime (BYO Docker
//! containers). It never supervises the app: it starts the gateway, injects
//! ALIEN_AI_GATEWAY_URL, and `exec`s the app so the app runs as the main process.
//!
//! Modes:
//!   alien-ai-gateway --gateway-serve       run the gateway forever (fixed port)
//!   alien-ai-gateway -- <cmd> [args...]     bootstrap, then exec <cmd>

use std::net::{Ipv4Addr, SocketAddr};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

const DEFAULT_PORT: u16 = 9008;
const PORT_ENV: &str = "ALIEN_AI_GATEWAY_PORT";
const URL_ENV: &str = "ALIEN_AI_GATEWAY_URL";
const SERVE_FLAG: &str = "--gateway-serve";

fn port() -> u16 {
    match std::env::var(PORT_ENV) {
        Err(_) => DEFAULT_PORT,
        // Set but unparseable: fail loud rather than silently binding a different port.
        Ok(v) => v
            .parse()
            .unwrap_or_else(|_| die(&format!("{PORT_ENV}='{v}' is not a valid port"))),
    }
}

fn die(msg: &str) -> ! {
    eprintln!("alien-ai-gateway: {msg}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.get(1).map(String::as_str) == Some(SERVE_FLAG) {
        serve_forever();
    }

    let app_cmd = match args.iter().position(|a| a == "--") {
        Some(i) if i + 1 < args.len() => &args[i + 1..],
        _ => die("usage: alien-ai-gateway -- <command> [args...]"),
    };

    let bindings = alien_gateway::bindings_from_env()
        .unwrap_or_else(|e| die(&format!("could not read the AI bindings: {e}")));
    if bindings.is_empty() {
        // No ambient AI binding (or BYO-key only): run the app directly, zero overhead.
        exec_app(app_cmd);
    }

    // exec_app replaces this process image, so the gateway can't run in-process — it
    // wouldn't survive the exec. Spawn a separate copy of ourselves to serve it, wait
    // until it's ready, then exec the app.
    let self_exe =
        std::env::current_exe().unwrap_or_else(|e| die(&format!("cannot resolve own path: {e}")));
    let mut child = Command::new(self_exe)
        .arg(SERVE_FLAG)
        .stdin(Stdio::null())
        .spawn()
        .unwrap_or_else(|e| die(&format!("failed to start gateway child: {e}")));

    let base = format!("http://127.0.0.1:{}", port());
    if !alien_gateway::wait_until_ready_blocking(&base) {
        // Surface the child's own startup failure (e.g. an unavailable ambient
        // credential) rather than only a generic readiness timeout.
        if let Ok(Some(status)) = child.try_wait() {
            die(&format!("gateway process exited before ready ({status})"));
        }
        die(&format!("gateway at {base} did not become ready"));
    }
    std::env::set_var(URL_ENV, &base);
    exec_app(app_cmd);
}

fn serve_forever() -> ! {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| die(&format!("tokio runtime: {e}")));
    rt.block_on(async {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port()));
        let bindings = alien_gateway::bindings_from_env()
            .unwrap_or_else(|e| die(&format!("could not read the AI bindings: {e}")));
        let _handle = alien_gateway::start_gateway_on(bindings, addr)
            .await
            .unwrap_or_else(|e| die(&format!("gateway failed to start: {e}")));
        // Hold the process (and the server task) open for the container lifetime.
        std::future::pending::<()>().await;
    });
    unreachable!()
}

fn exec_app(app_cmd: &[String]) -> ! {
    let err = Command::new(&app_cmd[0]).args(&app_cmd[1..]).exec();
    die(&format!("failed to exec {}: {err}", app_cmd[0]));
}
