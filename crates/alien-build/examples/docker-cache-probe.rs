//! Synthetic Docker cache validation driver. See scripts/docker-cache/README.md.
use alien_build::toolchain::{
    Toolchain, ToolchainContext, WorkloadKind, cache_utils::store_from_url, docker::DockerToolchain,
};
use alien_core::BinaryTarget;
use std::{collections::HashMap, path::PathBuf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: docker-cache-probe SOURCE OUTPUT".into());
    }
    let output = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&output)?;
    let context = ToolchainContext {
        src_dir: std::fs::canonicalize(&args[1])?,
        build_dir: output,
        cache_store: store_from_url(std::env::var("PROBE_CACHE_URL").ok().as_deref())?,
        cache_prefix: std::env::var("PROBE_CACHE_SCOPE")
            .unwrap_or_else(|_| "synthetic-dependencies".into()),
        build_target: BinaryTarget::LinuxX64,
        runtime_platform_name: "local".into(),
        debug_mode: false,
        pull_base_images: false,
        workload: WorkloadKind::Container,
    };
    let toolchain = DockerToolchain {
        dockerfile: None,
        build_args: std::env::var("PROBE_DEP_VERSION")
            .ok()
            .map(|value| HashMap::from([("DEP_VERSION".into(), value)])),
        target: None,
    };
    let start = std::time::Instant::now();
    let result = toolchain.build(&context).await?;
    println!(
        "build_seconds={} output={result:?}",
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
