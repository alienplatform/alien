//! Run only in a disposable VM or isolated privileged container: changes cgroups and firewall.
use std::{
    fs,
    io::Write,
    net::{TcpListener, UdpSocket},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::Command,
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
