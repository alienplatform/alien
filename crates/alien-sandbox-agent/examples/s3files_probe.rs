//! Attaches an S3 Files access point the way the agent's helper does, for trying the mount on a
//! plain Linux host (as root, with the proxy at /opt/alien/efs-proxy and the CA bundle at
//! /opt/alien/efs-utils.crt). Credentials come from AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY and
//! optionally AWS_SESSION_TOKEN. Prints one line per step and no credential.
//!
//! usage: s3files_probe <mount-path> <file-system-id> <access-point-id> <region> <zone-id> [renew-seconds]

#[cfg(target_os = "linux")]
fn main() {
    use alien_sandbox_agent::mounts::{write_credentials, MountCredentials};
    use alien_sandbox_agent::s3files_mount::Helper;
    use std::time::Instant;

    tracing_subscriber::fmt::init();
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 6 {
        eprintln!("usage: s3files_probe <mount-path> <file-system-id> <access-point-id> <region>");
        std::process::exit(2);
    }
    let (path, fs, ap, region) = (&args[1], &args[2], &args[3], &args[4]);
    let credentials = || MountCredentials {
        access_key_id: std::env::var("AWS_ACCESS_KEY_ID").expect("AWS_ACCESS_KEY_ID"),
        secret_access_key: std::env::var("AWS_SECRET_ACCESS_KEY").expect("AWS_SECRET_ACCESS_KEY"),
        session_token: std::env::var("AWS_SESSION_TOKEN").unwrap_or_default(),
        expiration: "2999-01-01T00:00:00Z".into(),
    };
    let dir = std::path::PathBuf::from("/run/alien/mounts");
    write_credentials(&dir, path, &credentials()).expect("store credentials");
    let mut helper = Helper::new(dir.clone());
    let started = Instant::now();
    match helper.mount(path, fs, ap, region, args.get(5).map(String::as_str)) {
        Ok(()) => println!("mounted in {} ms", started.elapsed().as_millis()),
        Err(reason) => {
            println!("failed: {reason}");
            std::process::exit(1);
        }
    }
    let marker = format!("{path}/probe-{}.txt", std::process::id());
    match std::fs::write(&marker, b"written through the helper\n") {
        Ok(()) => println!("write ok"),
        Err(e) => println!("write failed: {e}"),
    }
    write_credentials(&dir, path, &credentials()).expect("store credentials");
    println!("refresh: {:?}", helper.refresh(path));
    match std::fs::read_to_string(&marker) {
        Ok(text) => println!("read after refresh ok ({} bytes)", text.len()),
        Err(e) => println!("read after refresh failed: {e}"),
    }
    if let Some(seconds) = args.get(6).and_then(|s| s.parse::<u64>().ok()) {
        std::thread::sleep(std::time::Duration::from_secs(seconds));
    }
    helper.stop_proxy(path);
    println!("proxy stopped; the mount stays until unmounted");
}

#[cfg(not(target_os = "linux"))]
fn main() {}
