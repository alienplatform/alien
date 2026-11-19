//! The helper's S3 Files work: start the proxy, make the client certificate, mount over the
//! proxy's loopback port, and refresh the certificate when credentials are renewed.
//!
//! Runs only in the helper process, which was forked at boot with `CAP_SYS_ADMIN`. Everything it
//! executes lives under `/opt/alien`, which the image cannot write.

use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use chrono::Utc;

use crate::mounts::{c_string, credentials_file, MountCredentials};
use crate::s3files::{self, ClientKey, ProxyFiles};

/// The proxy shipped with the agent bundle.
pub const PROXY_BINARY: &str = "/opt/alien/efs-proxy";
/// Amazon's root certificates, shipped with the proxy (efs-utils `dist/efs-utils.crt`).
pub const CA_BUNDLE: &str = "/opt/alien/efs-utils.crt";

struct Attached {
    dir: PathBuf,
    proxy: Child,
    fs_id: String,
    access_point_id: String,
    region: String,
}

pub struct Helper {
    credentials_dir: PathBuf,
    key: Option<ClientKey>,
    attached: HashMap<String, Attached>,
}

impl Helper {
    pub fn new(credentials_dir: PathBuf) -> Self {
        Self {
            credentials_dir,
            key: None,
            attached: HashMap::new(),
        }
    }

    fn key(&mut self) -> Result<&ClientKey, String> {
        if self.key.is_none() {
            self.key = Some(ClientKey::generate()?);
        }
        Ok(self.key.as_ref().expect("just set"))
    }

    fn read_credentials(&self, mount_path: &str) -> Result<MountCredentials, String> {
        let file = credentials_file(&self.credentials_dir, mount_path);
        let text = std::fs::read_to_string(file).map_err(|_| "no credentials stored".to_string())?;
        serde_json::from_str(&text).map_err(|_| "stored credentials are unreadable".to_string())
    }

    fn write_certificate(
        &mut self,
        dir: &Path,
        credentials: &MountCredentials,
        fs_id: &str,
        access_point_id: &str,
        region: &str,
    ) -> Result<(), String> {
        let pem = {
            let key = self.key()?;
            s3files::client_certificate_pem(
                key,
                credentials,
                region,
                fs_id,
                access_point_id,
                "alien-sandbox",
                Utc::now(),
            )?
        };
        let staging = dir.join("certificate.pem.tmp");
        let _ = std::fs::remove_file(&staging);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staging)
            .map_err(|_| "cannot write the certificate".to_string())?;
        std::io::Write::write_all(&mut file, pem.as_bytes())
            .map_err(|_| "cannot write the certificate".to_string())?;
        std::fs::rename(&staging, dir.join("certificate.pem"))
            .map_err(|_| "cannot write the certificate".to_string())
    }

    pub fn mount(
        &mut self,
        path: &str,
        fs_id: &str,
        access_point_id: &str,
        region: &str,
        availability_zone_id: Option<&str>,
    ) -> Result<(), String> {
        let zone = availability_zone_id
            .ok_or_else(|| "the mount names no availability zone id".to_string())?;
        if self.attached.contains_key(path) {
            return Err("already mounted".into());
        }
        tracing::info!(mount = path, "reading the stored credentials");
        let credentials = self.read_credentials(path)?;
        let dir = self.credentials_dir.join(format!(
            "proxy-{}",
            credentials_file(&self.credentials_dir, path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("x")
        ));
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|_| "cannot create the proxy directory".to_string())?;

        tracing::info!(mount = path, "preparing the client key");
        // The private key: written once per mount from the in-memory key, readable by root only.
        let key_pem = self.key()?.pkcs8_pem();
        let key_path = dir.join("key.pem");
        let _ = std::fs::remove_file(&key_path);
        let mut key_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o400)
            .open(&key_path)
            .map_err(|_| "cannot write the client key".to_string())?;
        std::io::Write::write_all(&mut key_file, key_pem.as_bytes())
            .map_err(|_| "cannot write the client key".to_string())?;
        tracing::info!(mount = path, "writing the client certificate");
        self.write_certificate(&dir, &credentials, fs_id, access_point_id, region)?;

        // The port is chosen here and handed to the proxy as an open socket, as upstream does, so
        // nothing else can take it between choosing and listening.
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|_| "cannot reserve a loopback port".to_string())?;
        let port = listener
            .local_addr()
            .map_err(|_| "cannot read the loopback port".to_string())?
            .port();
        let dns_name = s3files::mount_target_name(fs_id, region, zone);
        let config = s3files::proxy_config(&ProxyFiles {
            listen_port: port,
            dns_name: &dns_name,
            fs_id,
            region,
            ca_file: CA_BUNDLE,
            certificate: dir.join("certificate.pem").to_str().unwrap_or_default(),
            private_key: key_path.to_str().unwrap_or_default(),
            pid_file: dir.join("proxy.pid").to_str().unwrap_or_default(),
            log_file: dir.join("proxy.log").to_str().unwrap_or_default(),
        });
        let config_path = dir.join("proxy.conf");
        std::fs::write(&config_path, config).map_err(|_| "cannot write the proxy config".to_string())?;

        let fd = listener.as_raw_fd();
        let mut command = Command::new(PROXY_BINARY);
        command
            .args(s3files::proxy_arguments(config_path.to_str().unwrap_or_default()))
            .args(["--listen-fd", &fd.to_string()])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .mode(0o600)
                    .open(dir.join("proxy.log"))
                    .map(Stdio::from)
                    .unwrap_or_else(|_| Stdio::null()),
            );
        // SAFETY: only async-signal-safe calls between fork and exec: the listening socket
        // must survive the exec, so its close-on-exec flag is cleared.
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        tracing::info!(mount = path, port, "starting the proxy");
        let mut proxy = command
            .spawn()
            .map_err(|e| format!("cannot start the proxy: {e}"))?;
        drop(listener);
        // A proxy that dies at once has a reason in its log; say only that it did.
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Ok(Some(status)) = proxy.try_wait() {
            return Err(format!("the proxy exited at start ({status})"));
        }

        std::fs::create_dir_all(path).map_err(|e| format!("cannot create {path}: {e}"))?;
        tracing::info!(mount = path, port, "mounting over the proxy");
        let outcome = mount_nfs(path, port);
        if let Err(reason) = outcome {
            let _ = proxy.kill();
            let _ = proxy.wait();
            return Err(reason);
        }
        self.attached.insert(
            path.to_string(),
            Attached {
                dir,
                proxy,
                fs_id: fs_id.to_string(),
                access_point_id: access_point_id.to_string(),
                region: region.to_string(),
            },
        );
        Ok(())
    }

    /// Makes a new certificate from the stored credentials and has the proxy read it (upstream's
    /// watchdog does the same with SIGHUP). The mount stays as it is.
    pub fn refresh(&mut self, path: &str) -> Result<(), String> {
        let (dir, fs_id, access_point_id, region, pid) = match self.attached.get(path) {
            Some(a) => (
                a.dir.clone(),
                a.fs_id.clone(),
                a.access_point_id.clone(),
                a.region.clone(),
                a.proxy.id() as i32,
            ),
            None => return Err("not mounted".into()),
        };
        let credentials = self.read_credentials(path)?;
        self.write_certificate(&dir, &credentials, &fs_id, &access_point_id, &region)?;
        // SAFETY: signalling the proxy this helper started.
        if unsafe { libc::kill(pid, libc::SIGHUP) } != 0 {
            return Err("the proxy is gone".into());
        }
        Ok(())
    }

    /// Stops the proxy of a mount that is being unmounted.
    pub fn stop_proxy(&mut self, path: &str) {
        if let Some(mut attached) = self.attached.remove(path) {
            // The unmount comes first in the caller; a proxy that outlives it only holds a port.
            let _ = attached.proxy.kill();
            let _ = attached.proxy.wait();
        }
    }
}

fn mount_nfs(path: &str, port: u16) -> Result<(), String> {
    let source = c_string("127.0.0.1:/")?;
    let target = c_string(path)?;
    let fstype = c_string("nfs4")?;
    // `addr` and `clientaddr` are what mount.nfs4 would have worked out for a loopback server.
    let options = c_string(&format!(
        "{},addr=127.0.0.1,clientaddr=127.0.0.1,proto=tcp",
        s3files::nfs_options(port)
    ))?;
    // SAFETY: valid NUL-terminated strings that outlive the call.
    let rc = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fstype.as_ptr(),
            libc::MS_NOSUID | libc::MS_NODEV,
            options.as_ptr().cast(),
        )
    };
    if rc != 0 {
        return Err(format!(
            "nfs mount failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}
