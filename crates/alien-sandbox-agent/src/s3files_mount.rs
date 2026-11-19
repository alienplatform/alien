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
/// One mount, from the first step to the mounted file system, gets this long. A step that does
/// not finish fails the mount with the step's name.
pub const MOUNT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(20);
const DNS_BUDGET: std::time::Duration = std::time::Duration::from_secs(6);
const CONNECT_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

/// Runs `work` on its own thread and gives up after `budget`. The thread is left to finish by
/// itself when it overruns (a blocked system call cannot be cancelled).
fn within<T: Send + 'static>(
    budget: std::time::Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver.recv_timeout(budget).ok()
}

/// The last line the proxy wrote, printable and short, for a failure message.
fn proxy_log_tail(dir: &Path) -> String {
    let text = std::fs::read_to_string(dir.join("proxy.log")).unwrap_or_default();
    let line = text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
    let line: String = line.chars().filter(|c| !c.is_control()).take(200).collect();
    if line.is_empty() {
        "the proxy log is empty".to_string()
    } else {
        format!("proxy log: {line}")
    }
}

struct Attached {
    dir: PathBuf,
    proxy: Child,
    fs_id: String,
    access_point_id: String,
    region: String,
    port: u16,
}

pub struct Helper {
    credentials_dir: PathBuf,
    key: Option<ClientKey>,
    /// Loopback ports of the running proxies, which guest sockets must not reach.
    ports: Vec<u16>,
    attached: HashMap<String, Attached>,
}

impl Helper {
    pub fn new(credentials_dir: PathBuf) -> Self {
        Self {
            credentials_dir,
            key: None,
            ports: Vec::new(),
            attached: HashMap::new(),
        }
    }

    /// Drops guest-marked traffic to the NFS port and to every proxy port. Fails closed: a mount
    /// is not made when the rules cannot be installed.
    fn guard_ports(&self) -> Result<(), String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "cannot start the network rule installer".to_string())?;
        runtime
            .block_on(crate::mount_network::install(&self.ports))
            .map_err(|_| "cannot install the mount network rules".to_string())
    }

    /// Makes the client key ahead of the first mount. A failure is left for the mount to report.
    pub fn prepare_key(&mut self) {
        let _ = self.key();
    }

    fn key(&mut self) -> Result<&ClientKey, String> {
        if self.key.is_none() {
            self.key = Some(ClientKey::generate()?);
        }
        Ok(self.key.as_ref().expect("just set"))
    }

    fn read_credentials(&self, mount_path: &str) -> Result<MountCredentials, String> {
        let file = credentials_file(&self.credentials_dir, mount_path);
        let text =
            std::fs::read_to_string(file).map_err(|_| "no credentials stored".to_string())?;
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
        let zone = availability_zone_id.ok_or_else(|| {
            "the mount names no availabilityZoneId: pass the zone id of a mount target of this \
                 file system that the sandbox's network reaches (for example use2-az1; the S3 \
                 Files mount target listing shows it)"
                .to_string()
        })?;
        if self.attached.contains_key(path) {
            return Err("already mounted".into());
        }
        let started = std::time::Instant::now();
        // Step 1, dns: the mount target's name must resolve (and the network must let this
        // process ask). Step 2, connect: its NFS port must answer.
        let dns_name = s3files::mount_target_name(fs_id, region, zone);
        tracing::info!(mount = path, "resolving the mount target");
        let lookup_name = dns_name.clone();
        let address = within(DNS_BUDGET, move || {
            use std::net::ToSocketAddrs;
            (lookup_name.as_str(), crate::mount_network::NFS_PORT)
                .to_socket_addrs()
                .map(|mut a| a.next())
                .map_err(|e| e.to_string())
        });
        let address = match address {
            None => {
                return Err(format!(
                    "dns: resolving {dns_name} took longer than {} s (the sandbox network gives this process no resolver)",
                    DNS_BUDGET.as_secs()
                ))
            }
            Some(Err(e)) => return Err(format!("dns: cannot resolve {dns_name}: {e}")),
            Some(Ok(None)) => return Err(format!("dns: {dns_name} has no address")),
            Some(Ok(Some(address))) => address,
        };
        tracing::info!(mount = path, "connecting to the mount target");
        if let Err(e) = std::net::TcpStream::connect_timeout(&address, CONNECT_BUDGET) {
            return Err(format!(
                "connect: cannot reach the mount target {address} on port {}: {e}",
                crate::mount_network::NFS_PORT
            ));
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
        // The rules come before the proxy exists, so there is no moment it is reachable by guests.
        self.ports.push(port);
        if let Err(reason) = self.guard_ports() {
            self.ports.pop();
            return Err(reason);
        }
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
        std::fs::write(&config_path, config)
            .map_err(|_| "cannot write the proxy config".to_string())?;

        let fd = listener.as_raw_fd();
        let mut command = Command::new(PROXY_BINARY);
        command
            .args(s3files::proxy_arguments(
                config_path.to_str().unwrap_or_default(),
            ))
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
        let mut proxy = match command.spawn() {
            Ok(proxy) => proxy,
            Err(e) => {
                self.ports.retain(|p| *p != port);
                return Err(format!("proxy: cannot start the proxy: {e}"));
            }
        };
        drop(listener);
        // A proxy that dies at once has a reason in its log; say only that it did.
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Ok(Some(status)) = proxy.try_wait() {
            self.ports.retain(|p| *p != port);
            return Err(format!(
                "proxy: exited at start ({status}); {}",
                proxy_log_tail(&dir)
            ));
        }

        std::fs::create_dir_all(path).map_err(|e| format!("cannot create {path}: {e}"))?;
        tracing::info!(mount = path, port, "mounting over the proxy");
        // Step 3, tls and nfs: the kernel's mount call waits on the proxy's TLS session to the
        // mount target, and does not return on its own when that never comes up.
        let target = path.to_string();
        let left = MOUNT_DEADLINE
            .saturating_sub(started.elapsed())
            .max(std::time::Duration::from_secs(5));
        let outcome = within(left, move || mount_nfs(&target, port));
        let outcome = match outcome {
            Some(result) => result.map_err(|e| format!("nfs: {e}; {}", proxy_log_tail(&dir))),
            None => Err(format!(
                "tls/nfs: the mount did not finish in {} s; {}",
                MOUNT_DEADLINE.as_secs(),
                proxy_log_tail(&dir)
            )),
        };
        if let Err(reason) = outcome {
            let _ = proxy.kill();
            let _ = proxy.wait();
            self.ports.retain(|p| *p != port);
            let _ = self.guard_ports();
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
                port,
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
            self.ports.retain(|p| *p != attached.port);
            let _ = self.guard_ports();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn a_step_that_overruns_is_given_up_on() {
        let started = Instant::now();
        let result = within(Duration::from_millis(200), || {
            std::thread::sleep(Duration::from_secs(3));
            1
        });
        assert!(result.is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(within(Duration::from_secs(2), || 7), Some(7));
    }

    #[test]
    fn the_proxy_log_tail_is_one_short_printable_line() {
        let dir = std::env::temp_dir().join(format!("tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(proxy_log_tail(&dir).contains("empty"));
        std::fs::write(dir.join("proxy.log"), "first\nTLS \u{1b}[31mhandshake failed\n\n").unwrap();
        let tail = proxy_log_tail(&dir);
        assert_eq!(tail, "proxy log: TLS [31mhandshake failed");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unresolvable_mount_target_fails_fast_naming_the_dns_step() {
        let dir = std::env::temp_dir().join(format!("dns-{}", std::process::id()));
        let mut helper = Helper::new(dir);
        let started = Instant::now();
        let error = helper
            .mount("/mnt/x", "fs-0000", "fsap-0000", "us-east-2", Some("use2-az1"))
            .expect_err("nothing resolves here");
        assert!(error.starts_with("dns:") || error.starts_with("connect:"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(12));
    }
}
