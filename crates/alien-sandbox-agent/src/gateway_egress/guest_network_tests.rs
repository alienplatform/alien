//! Run only in a disposable VM or isolated privileged container: changes cgroups and firewall.
use std::{
    collections::HashMap,
    fs,
    io::Write,
    net::{TcpListener, UdpSocket},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use crate::{exec::ExecIdentity, privilege};

#[test]
#[ignore = "requires isolated network/mount/cgroup namespaces, ALL capabilities, cgroup2 and Python 3"]
fn root_and_child_uids_keep_redirects_and_cannot_escape() {
    super::guest_network::install(true).expect("install guest socket classification");
    privilege::block_command_ipv6();
    super::redirect::install().expect("install real gateway redirects");
    let tcp = TcpListener::bind("127.0.0.1:15001").expect("transparent TCP listener");
    let dns = UdpSocket::bind("127.0.0.1:15053").expect("synthetic DNS listener");
    // A live orchestrator may suspend/resume this same process between rounds.
    let barrier = std::env::var_os("ALIEN_ROUTING_RESUME_BARRIER").map(PathBuf::from);
    let rounds = if barrier.is_some() { 2 } else { 1 };
    let budget = std::time::Duration::from_secs(if barrier.is_some() { 180 } else { 15 });
    // Keep listeners bounded even when testing a broken parent implementation.
    tcp.set_nonblocking(true).unwrap();
    dns.set_read_timeout(Some(budget)).unwrap();
    let tcp_thread = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + budget;
        for _ in 0..3 * rounds {
            loop {
                match tcp.accept() {
                    Ok((mut socket, _)) => {
                        socket
                            .set_write_timeout(Some(std::time::Duration::from_secs(2)))
                            .unwrap();
                        socket.write_all(b"redirected TCP").unwrap();
                        break;
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "guest TCP bypassed redirect"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept redirected TCP: {error}"),
                }
            }
        }
    });
    let dns_thread = std::thread::spawn(move || {
        let mut packet = [0; 64];
        for _ in 0..3 * rounds {
            let (size, peer) = loop {
                match dns.recv_from(&mut packet) {
                    Ok(received) => break received,
                    // Linux may interrupt a blocked socket syscall during freeze/resume.
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => panic!("guest DNS did not reach local resolver: {error}"),
                }
            };
            assert_eq!(&packet[..size], b"test DNS");
            dns.send_to(b"redirected DNS", peer).unwrap();
        }
    });
    for round in 0..rounds {
        if round == 1 {
            let barrier = barrier.as_ref().unwrap();
            fs::write(barrier.with_extension("ready"), b"ready").unwrap();
            let deadline = std::time::Instant::now() + budget;
            while !barrier.exists() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "resume barrier timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        for uid in [0, 2000, 60000] {
            let mut child = Command::new("python3");
            child.args(["-c", r#"
import ctypes, errno, json, os, socket, struct, subprocess
from pathlib import Path
s = socket.socket()
s.settimeout(2)
assert s.getsockopt(socket.SOL_SOCKET, socket.SO_MARK) == 0xa11f
try:
    s.setsockopt(socket.SOL_SOCKET, socket.SO_MARK, 0)
except OSError as error:
    assert error.errno == errno.EPERM
else:
    raise AssertionError('guest changed socket mark')
try:
    Path('/sys/fs/cgroup/cgroup.procs').write_text(str(os.getpid()))
except OSError as error:
    assert error.errno in (errno.EROFS, errno.EACCES)
else:
    raise AssertionError('guest escaped its cgroup')
s.connect(('192.0.2.1', 443))
assert s.recv(64) == b'redirected TCP'
s.close()
with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as dns:
    dns.settimeout(2)
    dns.connect(('8.8.8.8', 53))
    dns.send(b'test DNS')
    assert dns.recv(64) == b'redirected DNS'
if os.getuid() == 0:
    attack = r"""
import ctypes, errno, os, socket, struct, subprocess
from pathlib import Path
Path('/tmp/cgroup-escape').mkdir(exist_ok=True)
subprocess.run(['mount', '-t', 'cgroup2', 'none', '/tmp/cgroup-escape'], check=True)
Path('/tmp/cgroup-escape/cgroup.procs').write_text(str(os.getpid()))
Path('/tmp/cgroup-escape/child').mkdir(exist_ok=True)
Path('/tmp/cgroup-escape/child/cgroup.procs').write_text(str(os.getpid()))
fd = os.open('/tmp/cgroup-escape', os.O_RDONLY)
attr = ctypes.create_string_buffer(struct.pack('<IIIII', fd, 0, 2, 0, 0))
libc = ctypes.CDLL(None, use_errno=True)
# SYS_bpf differs between the two Linux architectures supported by this test.
nr = 280 if os.uname().machine == 'aarch64' else 321
assert libc.syscall(nr, 9, ctypes.byref(attr), 20) == -1
assert ctypes.get_errno() == errno.EPERM, 'BPF detach was not denied'
with socket.socket() as s:
    assert s.getsockopt(socket.SOL_SOCKET, socket.SO_MARK) == 0xa11f
"""
    subprocess.run(['unshare', '-UrCm', 'python3', '-c', attack], check=True)
print(json.dumps({'uid':os.getuid(), 'tcpRedirect':True, 'dnsRedirect':True, 'markChangeDenied':True, 'cgroupEscapeDenied':True}))
"#]);
            unsafe {
                child.pre_exec(move || privilege::drop_image_to(ExecIdentity { uid, gid: uid }));
            }
            let output = child.output().expect("spawn bounded image identity");
            println!("{}", String::from_utf8_lossy(&output.stdout));
            assert!(
                output.status.success(),
                "uid {uid}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    tcp_thread
        .join()
        .expect("every guest TCP connection redirected");
    dns_thread
        .join()
        .expect("every guest DNS request redirected");
}

#[test]
#[ignore = "requires isolated mount/cgroup namespaces, ALL capabilities and Python 3"]
fn device_allowlist_denies_raw_commands_and_preserves_terminals() {
    let directory = tempfile::tempdir().unwrap();
    // Create nodes before confinement too: denying mknod alone is insufficient.
    let nodes = Command::new("python3")
        .arg("-c")
        .arg(
            r#"
import os, stat, sys
for major, minor in [(1,1), (21,0), (10,236), (10,229), (240,0), (241,0)]:
    os.mknod(f'{sys.argv[1]}/{major}-{minor}', stat.S_IFCHR | 0o666, os.makedev(major,minor))
"#,
        )
        .arg(directory.path())
        .status()
        .unwrap();
    assert!(nodes.success());
    super::guest_network::install(false).unwrap();
    let repeated = super::guest_network::install(false).unwrap_err();
    assert_eq!(repeated.kind(), std::io::ErrorKind::AlreadyExists);
    assert!(repeated
        .to_string()
        .contains("bootstrap must install it only once"));
    for uid in [0, 60000] {
        let mut child = Command::new("python3");
        child
            .arg("-c")
            .arg(
                r#"
import errno, os, stat, sys, termios, tty
if os.getuid() == 0:
    for major, minor in [(1,1), (21,0), (10,236), (10,229), (240,0), (241,0)]:
        path=f'{sys.argv[1]}/{major}-{minor}'
        for flags in [os.O_RDONLY, os.O_WRONLY, os.O_RDWR]:
            try: fd=os.open(path,flags | os.O_NONBLOCK)
            except OSError as error: assert error.errno == errno.EPERM, (path,error)
            else: os.close(fd); raise AssertionError(('raw device open',path))
        try: os.mknod(path+'-new',stat.S_IFCHR | 0o600,os.makedev(major,minor))
        except OSError as error: assert error.errno == errno.EPERM, (path,error)
        else: raise AssertionError(('raw device mknod',path))
    for major, minor in [(1,3),(1,5),(1,7),(1,8),(1,9),(5,0),(5,1),(5,2),(136,0)]:
        path=f'{sys.argv[1]}/allowed-{major}-{minor}'
        os.mknod(path,stat.S_IFCHR | 0o600,os.makedev(major,minor))
        os.unlink(path)
with open('/dev/null','wb') as f: assert f.write(b'normal output') == 13
with open('/dev/zero','rb') as f: assert f.read(16) == bytes(16)
for path in ['/dev/random','/dev/urandom']:
    with open(path,'rb') as f: assert len(f.read(16)) == 16
try:
    fd=os.open('/dev/full',os.O_WRONLY)
    try: os.write(fd,b'x')
    finally: os.close(fd)
except OSError as error: assert error.errno == errno.ENOSPC
else: raise AssertionError('/dev/full accepted a write')
master,slave=os.openpty()
tty.setraw(slave)
assert os.major(os.fstat(slave).st_rdev) == 136
os.write(master,b'input'); assert os.read(slave,5)==b'input'
os.write(slave,b'output'); assert os.read(master,6)==b'output'
os.close(master); os.close(slave)
print(f'uid={os.getuid()} ordinary devices and PTY work; raw devices denied')
"#,
            )
            .arg(directory.path());
        // SAFETY: only async-signal-safe privilege syscalls execute before exec.
        unsafe {
            child.pre_exec(move || {
                privilege::restrict_supervisor(false)?;
                privilege::drop_image_to(ExecIdentity { uid, gid: uid })
            });
        }
        assert!(
            child.status().unwrap().success(),
            "device contract for UID {uid}"
        );
    }
}

#[test]
#[ignore = "requires isolated network/mount/cgroup namespaces, ALL capabilities, cgroup2, Python 3, the NFS client, and the remote network namespace scripts/test-sandbox-agent-security.sh sets up"]
fn image_nfs_leaves_only_through_its_mark_and_declared_connections() {
    // Off-VM addresses (10.0.1.5, 10.0.2.5, fd00::5) live in another network namespace behind a
    // veth pair; 192.168.77.1 is this namespace's own address on that link.
    let remote_namespace =
        std::env::var("ALIEN_TEST_REMOTE_NETNS").expect("ALIEN_TEST_REMOTE_NETNS");
    // Runs `make` on a thread in the namespace an address belongs to; the socket stays there.
    let bind_at = |address: &'static str| {
        let remote = address.starts_with("10.") || address.starts_with("[fd00:");
        let namespace = remote_namespace.clone();
        std::thread::spawn(move || {
            if remote {
                let file = fs::File::open(&namespace).expect("remote namespace");
                // SAFETY: a network namespace descriptor; only this thread changes namespace.
                let entered = unsafe {
                    libc::setns(std::os::fd::AsRawFd::as_raw_fd(&file), libc::CLONE_NEWNET)
                };
                assert_eq!(entered, 0, "{}", std::io::Error::last_os_error());
            }
            (TcpListener::bind(address), UdpSocket::bind(address))
        })
        .join()
        .unwrap()
    };
    // A declaration with one private connection, read the way the agent reads it at boot.
    std::env::set_var(
        crate::image_permissions::ENV_IMAGE_COMMAND,
        alien_core::sandbox_image::IMAGE_COMMAND_WITH_IMAGE_PERMISSIONS,
    );
    std::env::set_var(crate::image_permissions::ENV_PERMISSIONS_VERSION, "1");
    std::env::set_var(
        crate::image_permissions::ENV_PERMISSIONS,
        r#"{"mounts":[{"type":"nfs","path":"/workspace"}],"connections":[{"cidr":"10.0.1.5/32","port":2049}]}"#,
    );
    crate::image_permissions::load_from_env()
        .expect("declaration")
        .expect("declared");
    super::guest_network::install(true).expect("install guest socket classification");
    privilege::block_command_ipv6();
    super::redirect::install().expect("install real gateway redirects");
    // The gateway's redirect target, a server at a declared address, one at an undeclared
    // address, and a loopback NFS proxy. Each answers with its name and counts connections.
    let connections: Mutex<HashMap<&str, Arc<AtomicUsize>>> = Mutex::default();
    for (address, name) in [
        ("127.0.0.1:15001", "gateway"),
        ("10.0.1.5:2049", "declared"),
        ("10.0.2.5:2049", "undeclared"),
        ("127.0.0.1:2049", "loopback"),
        // Where a pNFS layout from the image's server could send the kernel client.
        ("10.0.1.5:20049", "data-server"),
        ("127.0.0.1:20049", "loopback-data-server"),
        ("[fd00::5]:20049", "data-server-v6"),
        ("[::1]:20049", "loopback-data-server-v6"),
        // The VM's own non-loopback address, where provider processes reach the agent.
        ("192.168.77.1:20049", "local-address"),
    ] {
        let listener = bind_at(address).0.expect(address);
        let count = Arc::clone(
            connections
                .lock()
                .unwrap()
                .entry(address)
                .or_insert_with(|| Arc::new(AtomicUsize::new(0))),
        );
        std::thread::spawn(move || {
            for mut socket in listener.incoming().flatten() {
                count.fetch_add(1, Ordering::SeqCst);
                let _ = socket.write_all(name.as_bytes());
            }
        });
    }
    // The image cgroup through a cgroup2 view of our own: the guest's is read-only now.
    let view = tempfile::tempdir().unwrap();
    let mounted = Command::new("mount")
        .args(["-t", "cgroup2", "none"])
        .arg(view.path())
        .status()
        .unwrap();
    assert!(mounted.success());
    let image_procs = view.path().join("alien-guest/image/cgroup.procs");
    assert!(image_procs.exists(), "the image cgroup exists");

    const PROBE: &str = r#"
import socket, sys
mark, host, port = sys.argv[1], sys.argv[2], int(sys.argv[3])
s = socket.socket()
s.settimeout(2)
if mark != 'own':
    s.setsockopt(socket.SOL_SOCKET, socket.SO_MARK, int(mark, 0))
print(hex(s.getsockopt(socket.SOL_SOCKET, socket.SO_MARK)), end=' ')
try:
    s.connect((host, port))
    print(s.recv(64).decode() or 'closed')
except socket.timeout:
    print('dropped')
except OSError as error:
    print(f'error {error.errno}')
"#;
    // `image`: a bounded root process in the image cgroup. `guest`: the same in the guest cgroup.
    // A number: a socket carrying that mark, set by a process that may (the kernel NFS client's
    // sockets carry no mark: 0).
    let probe = |who: &str, host: &str, port: u16| -> String {
        let mut child = Command::new("python3");
        let mark = if who == "image" || who == "guest" {
            "own"
        } else {
            who
        };
        child.args(["-c", PROBE, mark, host, &port.to_string()]);
        if who == "image" || who == "guest" {
            let procs = (who == "image").then(|| image_procs.clone());
            // SAFETY: only a write to cgroup.procs and privilege syscalls run before exec.
            unsafe {
                child.pre_exec(move || {
                    if let Some(procs) = &procs {
                        fs::write(procs, "0")?;
                    }
                    privilege::drop_image_to(ExecIdentity { uid: 0, gid: 0 })
                });
            }
        }
        let output = child.output().expect("probe");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    let image_mark = format!("{:#x}", crate::mount_broker::IMAGE_MARK);
    let cases = [
        // The image reaches its declared connection directly, and nothing else.
        ("image", "10.0.1.5", 2049, format!("{image_mark} declared")),
        ("image", "10.0.2.5", 2049, format!("{image_mark} gateway")),
        ("image", "10.0.1.5", 443, format!("{image_mark} gateway")),
        ("image", "127.0.0.1", 2049, format!("{image_mark} loopback")),
        // The rest of the guest goes through the gateway as before.
        ("guest", "10.0.1.5", 2049, "0xa11f gateway".to_string()),
        // Unmarked sockets (the kernel's NFS client) reach NFS only on loopback.
        ("0", "10.0.1.5", 2049, "0x0 dropped".to_string()),
        ("0", "10.0.2.5", 2049, "0x0 dropped".to_string()),
        ("0", "127.0.0.1", 2049, "0x0 loopback".to_string()),
        // Nor anywhere else off loopback (the kernel's helpers, outside the guest cgroup).
        ("0", "10.0.1.5", 20049, "0x0 dropped".to_string()),
        (
            "0",
            "127.0.0.1",
            20049,
            "0x0 loopback-data-server".to_string(),
        ),
        // The VM's own address stays reachable: provider processes reach the agent that way.
        ("0", "192.168.77.1", 20049, "0x0 local-address".to_string()),
        // The agent's own sockets carry its mark and are not affected.
        (
            "0xa11e",
            "10.0.1.5",
            20049,
            "0xa11e data-server".to_string(),
        ),
    ];
    let mut failures = Vec::new();
    for (who, host, port, expected) in cases {
        let got = probe(who, host, port);
        println!("{who:>5} -> {host}:{port}: {got}");
        if got != expected {
            failures.push(format!(
                "{who} -> {host}:{port}: got '{got}', expected '{expected}'"
            ));
        }
    }
    // UDP without a mark (a kernel helper's DNS, say) reaches only loopback; the agent's own
    // marked UDP is not affected.
    for (mark, address, expected) in [
        (0, "10.0.1.5:20053", "dropped"),
        (0, "[fd00::5]:20053", "dropped"),
        (0, "127.0.0.1:20053", "delivered"),
        (0, "[::1]:20053", "delivered"),
        (0, "192.168.77.1:20053", "delivered"),
        (super::redirect::MARK, "10.0.1.5:20053", "delivered"),
    ] {
        let receiver = bind_at(address).1.expect(address);
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        let sender = UdpSocket::bind(if address.starts_with('[') {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        })
        .unwrap();
        // SAFETY: a live socket and a correctly sized option.
        let marked = unsafe {
            libc::setsockopt(
                std::os::fd::AsRawFd::as_raw_fd(&sender),
                libc::SOL_SOCKET,
                libc::SO_MARK,
                (&mark as *const u32).cast(),
                size_of::<u32>() as _,
            )
        };
        assert_eq!(marked, 0, "{}", std::io::Error::last_os_error());
        let got = match sender
            .send_to(b"x", address)
            .and_then(|_| receiver.recv(&mut [0; 8]))
        {
            Ok(_) => "delivered",
            Err(_) => "dropped",
        };
        println!("udp {mark:#x} -> {address}: {got}");
        if got != expected {
            failures.push(format!(
                "udp {mark:#x} -> {address}: got '{got}', expected '{expected}'"
            ));
        }
    }
    // The kernel's NFS client itself (a socket the kernel made, which carries no mark) opens a connection only to
    // loopback, on any port and in IPv4 and IPv6. `mount -i` makes the mount(2) call directly.
    let target = tempfile::tempdir().unwrap();
    for (address, source, expected) in [
        ("10.0.1.5:20049", "10.0.1.5:/", "dropped"),
        ("127.0.0.1:20049", "127.0.0.1:/", "connected"),
        ("[fd00::5]:20049", "[fd00::5]:/", "dropped"),
        ("[::1]:20049", "[::1]:/", "connected"),
        ("192.168.77.1:20049", "192.168.77.1:/", "connected"),
    ] {
        let count = Arc::clone(&connections.lock().unwrap()[address]);
        let before = count.load(Ordering::SeqCst);
        let host = source.trim_end_matches(":/").trim_matches(['[', ']']);
        let mut mount = Command::new("mount")
            .args(["-i", "-t", "nfs4", "-o"])
            .arg(format!(
                "vers=4.2,proto={},addr={host},port=20049,soft,timeo=10,retrans=1",
                if host.contains(':') { "tcp6" } else { "tcp" }
            ))
            .arg(source)
            .arg(target.path())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("mount");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        while count.load(Ordering::SeqCst) == before && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = mount.kill();
        let _ = mount.wait();
        let got = if count.load(Ordering::SeqCst) > before {
            "connected"
        } else {
            "dropped"
        };
        println!("kernel -> {address}: {got}");
        if got != expected {
            failures.push(format!(
                "kernel NFS client -> {address}: got '{got}', expected '{expected}'"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
