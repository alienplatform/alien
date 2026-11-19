//! The broker against a real kernel NFS server, in a disposable privileged container
//! (`scripts/test-sandbox-mount-broker.sh`). One test: the broker is forked once per process.

use std::{
    io::{Read, Write},
    os::unix::{fs::symlink, process::CommandExt},
    path::Path,
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

use crate::{exec::ExecIdentity, image_permissions, mount_broker, privilege};

const DECLARATION: &str = r#"{
  "mounts": [
    {"type": "nfs", "path": "/workspace"},
    {"type": "tmpfs", "path": "/var/lib/vendor/workspace-tmp", "sizeBytes": 16777216},
    {"type": "bind", "from": "/var/lib/vendor/workspace-tmp", "path": "/workspace/tmp"},
    {"type": "nfs", "path": "/link"},
    {"type": "nfs", "path": "/linkparent/inner"},
    {"type": "tmpfs", "path": "/scratch", "sizeBytes": 1048576}
  ],
  "onPause": ["/bin/true"]
}"#;

/// A process in the image: the image cgroup (through the broker), root with the bounded grant,
/// and the image filters.
fn image_command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    // SAFETY: both calls are async-signal-safe and allocate nothing.
    unsafe {
        command.pre_exec(|| {
            mount_broker::join_image_cgroup()?;
            privilege::drop_image_to(ExecIdentity { uid: 0, gid: 0 })
        });
    }
    command
}

fn image(script: &str) -> Output {
    image_command("/bin/sh", &["-c", script])
        .stdin(Stdio::null())
        .output()
        .expect("run an image command")
}

fn spawn_image(script: &str) -> Child {
    image_command("/bin/sh", &["-c", script])
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn an image process")
}

fn text(output: &Output) -> String {
    format!(
        "status={:?} stdout={} stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[track_caller]
fn allowed(script: &str) -> Output {
    let output = image(script);
    assert!(output.status.success(), "{script}: {}", text(&output));
    println!("ALLOWED  {script}");
    output
}

/// Refused with exit 32 (mount.nfs's failure code) and `expect` in the message.
#[track_caller]
fn refused(script: &str, expect: &str) {
    let output = image(script);
    let message = text(&output);
    assert_eq!(output.status.code(), Some(32), "{script}: {message}");
    assert!(
        message.contains(expect),
        "{script}: wanted {expect:?}: {message}"
    );
    println!(
        "REFUSED  {script}  ->  {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
}

fn wait_for_listener(port: u16) {
    let until = Instant::now() + Duration::from_secs(10);
    while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
        assert!(Instant::now() < until, "nothing listens on {port}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn mount_line(path: &str) -> Option<String> {
    std::fs::read_to_string("/proc/self/mountinfo")
        .unwrap()
        .lines()
        .rev()
        .find(|line| line.split(' ').nth(4) == Some(path))
        .map(str::to_string)
}

const FORWARD: &str = r#"
import socket, sys, threading
port, target = int(sys.argv[1]), int(sys.argv[2])
delay = float(sys.argv[3]) if len(sys.argv) > 3 else 0
def pipe(a, b):
    try:
        while True:
            data = a.recv(65536)
            if not data: break
            b.sendall(data)
    except OSError: pass
    finally:
        for s in (a, b):
            try: s.shutdown(socket.SHUT_RDWR)
            except OSError: pass
listener = socket.socket(); listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(('127.0.0.1', port)); listener.listen(16)
while True:
    client, _ = listener.accept()
    if target == 0:
        threading.Thread(target=lambda c=client: c.recv(1 << 20) and __import__('time').sleep(3600), daemon=True).start()
        continue
    if delay:
        __import__('time').sleep(delay)
    upstream = socket.create_connection(('127.0.0.1', target))
    threading.Thread(target=pipe, args=(client, upstream), daemon=True).start()
    threading.Thread(target=pipe, args=(upstream, client), daemon=True).start()
"#;

#[test]
#[ignore = "requires a disposable privileged container with cgroup2, a kernel NFS server exporting a directory with hello.txt as / on 127.0.0.1:2049, python3, and the agent binary at /agent; run by scripts/test-sandbox-mount-broker.sh"]
fn broker_allows_declared_mounts_and_refuses_everything_else() {
    // The broker must be forked by a single-threaded process, as the agent does at boot; libtest
    // runs this test on a thread of its own, so the scenario runs in a forked child.
    // SAFETY: the child only runs the scenario and leaves with _exit.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork");
    if pid == 0 {
        let passed = std::panic::catch_unwind(scenario).is_ok();
        let _ = std::io::stdout().flush();
        // SAFETY: leaving the forked child.
        unsafe { libc::_exit(if passed { 0 } else { 1 }) };
    }
    let mut status = 0;
    // SAFETY: waiting for our own child.
    unsafe { libc::waitpid(pid, &mut status, 0) };
    assert!(
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
        "the scenario failed"
    );
}

fn scenario() {
    let declared = image_permissions::parse(DECLARATION).expect("declaration");
    std::fs::create_dir_all(mount_broker::IMAGE_CGROUP).expect("image cgroup");
    // The kernel's helper settings are host-wide and can only be lowered until reboot: the test
    // points the broker at stand-ins and checks what it writes.
    std::fs::create_dir_all("/tmp/helpers").unwrap();
    for name in ["bset", "inheritable"] {
        std::fs::write(format!("/tmp/helpers/{name}"), "4294967295\t511\n").unwrap();
    }
    std::fs::write("/tmp/helpers/modprobe", "/sbin/modprobe\n").unwrap();
    mount_broker::start(
        &declared,
        mount_broker::Config {
            deadline: Duration::from_secs(5),
            helpers: mount_broker::HelperSettings {
                bset: "/tmp/helpers/bset".into(),
                inheritable: "/tmp/helpers/inheritable".into(),
                modprobe: "/tmp/helpers/modprobe".into(),
            },
            ..Default::default()
        },
    )
    .expect("fork the broker");
    privilege::filter_image_mounts();
    std::fs::write("/proc/self/oom_score_adj", "0").ok();
    mount_broker::link_shims(Path::new("/agent")).expect("link the shims");
    let broker = mount_broker::pid().expect("broker pid");
    let capabilities = |pid: i32| {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
        let line = status.lines().find(|l| l.starts_with("CapEff:")).unwrap();
        u64::from_str_radix(line.split_whitespace().nth(1).unwrap(), 16).unwrap()
    };
    assert_ne!(
        capabilities(broker) & (1 << 16),
        0,
        "SYS_MODULE until the image starts"
    );
    // The socket's directory is reached without symbolic links.
    std::fs::create_dir_all("/tmp/link-target").unwrap();
    symlink("/tmp/link-target", "/tmp/linked-run").unwrap();
    let error = mount_broker::serve_socket(Path::new("/tmp/linked-run/alien/mount.sock"))
        .expect_err("a socket directory through a symbolic link");
    assert!(error.to_string().contains("symbolic link"), "{error}");
    assert!(!Path::new("/tmp/link-target/alien").exists());
    mount_broker::serve_socket(Path::new(mount_broker::SOCKET_PATH)).expect("serve the socket");

    // Sealed before the image starts: the kernel's helpers get no capabilities, no module
    // loading by program, and the broker no longer holds what that took.
    for name in ["bset", "inheritable"] {
        assert_eq!(
            std::fs::read_to_string(format!("/tmp/helpers/{name}")).unwrap(),
            "0\t0\n"
        );
    }
    assert_eq!(
        std::fs::read_to_string("/tmp/helpers/modprobe").unwrap(),
        "\n"
    );
    assert_eq!(
        capabilities(broker) & ((1 << 8) | (1 << 16)),
        0,
        "the broker dropped SETPCAP and SYS_MODULE"
    );
    assert!(
        mount_line("/usr/sbin").is_some_and(|line| line.contains(" ro,") || line.contains(" ro ")),
        "{:?}",
        mount_line("/usr/sbin")
    );
    for directory in [
        "/workspace",
        "/var/lib/vendor/workspace-tmp",
        "/scratch",
        "/elsewhere",
    ] {
        std::fs::create_dir_all(directory).unwrap();
    }
    std::fs::write("/tmp/forward.py", FORWARD).unwrap();

    // The image's loopback proxy, a foreign one (not in the image cgroup), a hung one and a slow
    // one (answers after 2s).
    let mut proxy = spawn_image("exec python3 /tmp/forward.py 20123 2049");
    let mut foreign = Command::new("python3")
        .args(["/tmp/forward.py", "20124", "2049"])
        .spawn()
        .unwrap();
    let mut hung = spawn_image("exec python3 /tmp/forward.py 20125 0");
    let mut slow = spawn_image("exec python3 /tmp/forward.py 20126 2049 2");
    for port in [20123, 20124, 20125, 20126] {
        wait_for_listener(port);
    }
    mount_broker::set_image_leader(proxy.id());
    // The traced options, but `soft`: a failed run must not leave a process waiting forever on
    // a mount whose proxy is gone. (`hard` is covered by the allowlist's unit tests.)
    let options = "nfsvers=4.2,port=20123,soft,timeo=20,retrans=1,noresvport,actimeo=30,rsize=1048576,wsize=1048576";

    // The image itself: root, no SYS_ADMIN, in the image cgroup.
    let status = allowed("grep -E '^CapEff' /proc/self/status; cat /proc/self/cgroup");
    let status = String::from_utf8_lossy(&status.stdout).to_string();
    let effective = u64::from_str_radix(
        status
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap(),
        16,
    )
    .unwrap();
    assert_eq!(
        effective & (1 << 21),
        0,
        "the image holds SYS_ADMIN: {status}"
    );
    assert!(status.contains("0::/alien-guest/image"), "{status}");

    // Image root cannot write, replace or move the kernel's helper directory.
    std::fs::write("/tmp/planted", "#!/bin/sh\n").unwrap();
    let attempts = allowed(
        r#"exec python3 -I -c '
import errno, os
def attempt(name, action):
    try:
        action()
        print(name, "OK")
    except OSError as error:
        print(name, errno.errorcode[error.errno])
attempt("write", lambda: open("/usr/sbin/request-key", "w"))
attempt("replace", lambda: os.rename(os.path.realpath("/usr/sbin/nologin"), "/usr/sbin/request-key"))
attempt("move-in", lambda: os.rename("/tmp/planted", "/usr/sbin/request-key"))
attempt("rename", lambda: os.rename("/usr/sbin", "/usr/sbin-moved"))
attempt("remove", lambda: os.unlink("/usr/sbin/nologin"))
'"#,
    );
    let attempts = String::from_utf8_lossy(&attempts.stdout).to_string();
    println!("{attempts}");
    for expected in [
        "write EROFS",
        "replace EROFS",
        "move-in EXDEV",
        "rename EBUSY",
        "remove EROFS",
    ] {
        assert!(
            attempts.lines().any(|l| l == expected),
            "{expected}: {attempts}"
        );
    }

    // Allowed: NFS through the image's proxy with the traced options; tmpfs; bind from it.
    allowed(&format!(
        "/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options} && cat /workspace/hello.txt && mkdir -p /workspace/tmp"
    ));
    let line = mount_line("/workspace").expect("the NFS mount");
    assert!(
        line.contains("nosuid") && line.contains("nodev") && line.contains(" nfs4 "),
        "{line}"
    );
    allowed("/opt/alien/bin/alien-mount tmpfs /var/lib/vendor/workspace-tmp -o size=8m,mode=770,uid=1001,gid=1001");
    let line = mount_line("/var/lib/vendor/workspace-tmp").expect("the tmpfs");
    assert!(
        line.contains("size=8192k") && line.contains("nosuid"),
        "{line}"
    );
    allowed("/opt/alien/bin/alien-mount bind /var/lib/vendor/workspace-tmp /workspace/tmp && echo x > /workspace/tmp/f && test -f /var/lib/vendor/workspace-tmp/f");
    assert!(mount_line("/workspace/tmp").is_some_and(|l| l.contains("nosuid")));

    // Wrong type, wrong path, too big.
    refused(
        "/opt/alien/bin/alien-mount tmpfs /workspace -o size=1m",
        "EPERM",
    );
    refused(
        &format!("/opt/alien/bin/mount.nfs4 127.0.0.1:/ /etc -o {options}"),
        "not a declared mount path",
    );
    refused(
        "/opt/alien/bin/alien-mount tmpfs /opt/alien -o size=1m",
        "not a declared mount path",
    );
    refused(
        "/opt/alien/bin/alien-mount tmpfs /scratch -o size=2m",
        "above the declared",
    );
    refused(
        "/opt/alien/bin/alien-mount bind /workspace /workspace/tmp",
        "may only be bound from",
    );
    // Address and port spoofing, v3, Kerberos, mountaddr.
    refused(
        &format!("/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options},addr=10.0.0.1"),
        "addr",
    );
    refused(
        &format!("/opt/alien/bin/mount.nfs4 10.0.0.1:/ /workspace -o {options}"),
        "loopback proxy",
    );
    refused(
        &format!(
            "/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options},clientaddr=10.0.0.9"
        ),
        "clientaddr",
    );
    refused(
        "/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o nfsvers=4.2,port=20124",
        "not the image's own proxy",
    );
    refused(
        "/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o nfsvers=4.2,port=2049",
        "loopback proxy port",
    );
    refused(
        "/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o nfsvers=3,port=20123",
        "vers",
    );
    refused(
        &format!("/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options},sec=krb5"),
        "sec",
    );
    refused(
        &format!(
            "/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options},mountaddr=10.0.0.1"
        ),
        "mountaddr",
    );
    refused(
        &format!("/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options},proto=udp"),
        "proto",
    );

    // Symbolic links: the target itself, or a parent.
    std::fs::create_dir_all("/elsewhere/inner").unwrap();
    symlink("/etc", "/link").unwrap();
    symlink("/elsewhere", "/linkparent").unwrap();
    refused(
        &format!("/opt/alien/bin/mount.nfs4 127.0.0.1:/ /link -o {options}"),
        "ELOOP",
    );
    refused(
        &format!("/opt/alien/bin/mount.nfs4 127.0.0.1:/ /linkparent/inner -o {options}"),
        "ELOOP",
    );
    assert!(mount_line("/etc").is_none() && mount_line("/elsewhere/inner").is_none());

    // A mount the broker did not make cannot be unmounted through it.
    assert!(Command::new("mount")
        .args(["-t", "tmpfs", "-o", "size=1m", "theirs", "/scratch"])
        .status()
        .unwrap()
        .success());
    refused(
        "/opt/alien/bin/alien-mount umount /scratch",
        "not made by the broker",
    );
    assert!(Command::new("umount")
        .arg("/scratch")
        .status()
        .unwrap()
        .success());

    // Rename race: the image renames the declared directory back and forth while asking for
    // mounts there. A mount either lands on /scratch or is removed; never elsewhere.
    let mut racer = image_command(
        "/bin/sh",
        &["-c", "while true; do mv -T /scratch /scratch-moved 2>/dev/null; mv -T /scratch-moved /scratch 2>/dev/null; done"],
    )
    .process_group(0)
    .spawn()
    .unwrap();
    let (mut landed, mut refusals) = (0, 0);
    for _ in 0..40 {
        let output = image("/opt/alien/bin/alien-mount tmpfs /scratch -o size=1m");
        if output.status.success() {
            landed += 1;
            if mount_line("/scratch").is_none() {
                let table = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
                let scratch: Vec<&str> = table.lines().filter(|l| l.contains("scratch")).collect();
                let names: Vec<_> = std::fs::read_dir("/")
                    .unwrap()
                    .flatten()
                    .map(|e| e.file_name())
                    .collect();
                panic!(
                    "a mount was reported at /scratch: {} {scratch:?} {names:?}",
                    text(&output)
                );
            }
            let output = image("/opt/alien/bin/alien-mount umount /scratch");
            assert!(output.status.success(), "{}", text(&output));
        } else {
            refusals += 1;
        }
        assert!(
            mount_line("/scratch-moved").is_none(),
            "a mount landed off the declared path"
        );
    }
    // SAFETY: signalling the racer's own process group.
    unsafe { libc::kill(-(racer.id() as i32), libc::SIGKILL) };
    racer.wait().unwrap();
    println!("RACE     40 tmpfs requests during renames: {landed} landed on /scratch, {refusals} refused, 0 elsewhere");
    let _ = std::fs::rename("/scratch-moved", "/scratch");

    // Peers: a root process outside the image cgroup, and a non-root image process.
    let mut outside = std::os::unix::net::UnixStream::connect(mount_broker::SOCKET_PATH).unwrap();
    outside
        .write_all(b"{\"op\":\"umount\",\"target\":\"/workspace\"}\n")
        .unwrap();
    let mut answer = String::new();
    outside.read_to_string(&mut answer).unwrap();
    assert!(
        answer.contains("only the image's own processes"),
        "{answer}"
    );
    println!(
        "REFUSED  root outside the image cgroup  ->  {}",
        answer.trim()
    );
    let mut lane = Command::new("/opt/alien/bin/alien-mount");
    lane.args(["umount", "/workspace"]);
    // SAFETY: async-signal-safe calls only.
    unsafe {
        lane.pre_exec(|| {
            mount_broker::join_image_cgroup()?;
            privilege::drop_image_to(ExecIdentity {
                uid: 1000,
                gid: 1000,
            })
        });
    }
    let output = lane.output().unwrap();
    assert_eq!(output.status.code(), Some(32), "{}", text(&output));
    assert!(
        text(&output).contains("Permission denied"),
        "{}",
        text(&output)
    );
    println!(
        "REFUSED  non-root image process  ->  {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );

    // Escapes the image filter closes.
    let probe = format!(
        r#"
import ctypes, errno, os
libc = ctypes.CDLL(None, use_errno=True)
def call(nr, *args):
    r = libc.syscall(nr, *[ctypes.c_long(a) for a in args])
    return 0 if r >= 0 else ctypes.get_errno()
results = {{}}
for name, nr in {enosys}.items():
    results[name] = errno.errorcode.get(call(nr, 0, 0, 0, 0, 0), 'OK')
for name, nr in {eperm}.items():
    results[name] = errno.errorcode.get(call(nr, 0, 0, 0, 0, 0), 'OK')
results['unshare(NEWUSER|NEWNS)'] = errno.errorcode.get(call({unshare}, 0x10000000 | 0x20000), 'OK')
results['unshare(NEWNS)'] = errno.errorcode.get(call({unshare}, 0x20000), 'OK')
results['mount'] = errno.errorcode.get(libc.mount(b'none', b'/tmp', b'tmpfs', 0, None) and ctypes.get_errno(), 'OK')
print(results)
"#,
        enosys = format!(
            "{{'fsopen': {}, 'fsconfig': {}, 'fsmount': {}, 'fspick': {}, 'move_mount': {}, 'open_tree': {}, 'mount_setattr': {}, 'pivot_root': {}, 'clone3': {}}}",
            libc::SYS_fsopen, libc::SYS_fsconfig, libc::SYS_fsmount, libc::SYS_fspick,
            libc::SYS_move_mount, libc::SYS_open_tree, libc::SYS_mount_setattr,
            libc::SYS_pivot_root, libc::SYS_clone3
        ),
        eperm = format!(
            "{{'setns': {}, 'request_key': {}, 'keyctl': {}, 'add_key': {}, 'perf_event_open': {}}}",
            libc::SYS_setns, libc::SYS_request_key, libc::SYS_keyctl, libc::SYS_add_key,
            libc::SYS_perf_event_open
        ),
        unshare = libc::SYS_unshare,
    );
    std::fs::write("/tmp/probe.py", probe).unwrap();
    let output = allowed("python3 /tmp/probe.py");
    let results = String::from_utf8_lossy(&output.stdout).to_string();
    println!("FILTER   {}", results.trim());
    for name in [
        "fsopen",
        "fsconfig",
        "fsmount",
        "fspick",
        "move_mount",
        "open_tree",
        "mount_setattr",
        "pivot_root",
        "clone3",
    ] {
        assert!(
            results.contains(&format!("'{name}': 'ENOSYS'")),
            "{name}: {results}"
        );
    }
    for name in [
        "setns",
        "request_key",
        "keyctl",
        "add_key",
        "perf_event_open",
        "unshare(NEWUSER|NEWNS)",
        "unshare(NEWNS)",
        "mount",
    ] {
        assert!(
            results.contains(&format!("'{name}': 'EPERM'")),
            "{name}: {results}"
        );
    }
    let output = image("unshare -Urm mount -t tmpfs none /scratch");
    assert!(
        !output.status.success(),
        "a user namespace mount: {}",
        text(&output)
    );
    println!(
        "REFUSED  unshare -Urm mount  ->  {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );

    // A hung server does not hang the broker: the request fails at the deadline and the next
    // one is answered at once.
    std::fs::create_dir_all("/workspace/tmp").ok();
    allowed("/opt/alien/bin/alien-mount umount /workspace/tmp && /opt/alien/bin/alien-mount umount /workspace");
    let started = Instant::now();
    refused(
        "/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o nfsvers=4.2,port=20125,soft,timeo=600",
        "ETIMEDOUT",
    );
    let took = started.elapsed();
    assert!(
        took < Duration::from_secs(12),
        "the hung mount took {took:?}"
    );
    let started = Instant::now();
    refused(
        "/opt/alien/bin/alien-mount tmpfs /workspace -o size=1m",
        "EPERM",
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    println!(
        "HUNG     the hung mount failed after {took:?}; the next request was answered at once"
    );

    // Lifecycle: the step runs in the image cgroup without SYS_ADMIN; a step past its deadline
    // has its whole process group killed. A failed pause step detaches the NFS mounts.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let step = |command: &[&str], deadline: u64| {
        let command: Vec<String> = command.iter().map(|s| s.to_string()).collect();
        runtime.block_on(crate::image_lifecycle::run(
            "onPause",
            &command,
            Duration::from_secs(deadline),
        ))
    };
    step(
        &["/bin/sh", "-c", "grep -q '^0::/alien-guest/image/steps$' /proc/self/cgroup && test \"$(id -u)\" = 0 && python3 -c \"import sys; v = int([l for l in open('/proc/self/status') if l.startswith('CapEff')][0].split()[1], 16); sys.exit(1 if v & (1 << 21) else 0)\""],
        10,
    )
    .expect("a step in the image");
    let started = Instant::now();
    // One child stays in the step's process group, one leaves it (setsid).
    let error = step(
        &["/bin/sh", "-c", "sleep 300 & setsid sleep 302 & sleep 301"],
        2,
    )
    .unwrap_err();
    assert!(error.contains("its processes were killed"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(300));
    let left = Command::new("pgrep")
        .args(["-f", "sleep 30[012]"])
        .output()
        .unwrap();
    assert!(
        left.stdout.is_empty(),
        "left behind: {}",
        String::from_utf8_lossy(&left.stdout)
    );
    println!("STEP     a step past its 2s deadline: {error}; nothing left behind");
    allowed(&format!(
        "/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options}"
    ));
    mount_broker::detach_nfs_mounts().expect("detach");
    assert!(
        mount_line("/workspace").is_none(),
        "the NFS mount is still there"
    );
    println!("PAUSE    a failed pause step detached the broker's NFS mount");

    // An NFS mount off every declared path (where a helper killed between attach and check
    // would leave one the image moved) goes at the pause detach too.
    std::fs::create_dir_all("/elsewhere/moved").unwrap();
    assert!(Command::new("mount")
        .args(["-i", "-t", "nfs4", "-o"])
        .arg(format!("{options},addr=127.0.0.1"))
        .args(["127.0.0.1:/", "/elsewhere/moved"])
        .status()
        .unwrap()
        .success());
    assert!(mount_line("/elsewhere/moved").is_some());
    mount_broker::detach_nfs_mounts().expect("detach");
    assert!(
        mount_line("/elsewhere/moved").is_none(),
        "an NFS mount off the declared paths survived the pause detach"
    );
    println!("PAUSE    an NFS mount off the declared paths was detached too");

    // From the pause until the resume, no new mounts.
    mount_broker::pausing().expect("pausing");
    refused(
        &format!("/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options}"),
        "pausing",
    );
    refused(
        "/opt/alien/bin/alien-mount tmpfs /scratch -o size=1m",
        "pausing",
    );
    mount_broker::resumed().expect("resumed");
    allowed("/opt/alien/bin/alien-mount tmpfs /scratch -o size=1m && /opt/alien/bin/alien-mount umount /scratch");
    println!("PAUSE    mounts were refused from the pause until the resume");

    // A proxy that starts listening just after the mount is asked for (a mount helper starts
    // it and mounts at once): the broker waits for it.
    let mut late = spawn_image("sleep 0.3; exec python3 /tmp/forward.py 20127 2049");
    allowed("/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o nfsvers=4.2,port=20127,soft,timeo=20,retrans=1 && cat /workspace/hello.txt && /opt/alien/bin/alien-mount umount /workspace");
    let _ = late.kill();
    let _ = late.wait();
    println!("LATE     a proxy listening 0.3s after the request was waited for");

    // At most four of the broker's mounts stacked on one path.
    allowed(
        &"/opt/alien/bin/alien-mount tmpfs /scratch -o size=1m && "
            .repeat(4)
            .trim_end_matches(" && ")
            .to_string(),
    );
    refused(
        "/opt/alien/bin/alien-mount tmpfs /scratch -o size=1m",
        "already has 4 mounts",
    );
    refused(
        "/opt/alien/bin/alien-mount tmpfs /scratch -o size=1m,nr_inodes=0",
        "nr_inodes",
    );
    allowed(
        &"/opt/alien/bin/alien-mount umount /scratch && "
            .repeat(4)
            .trim_end_matches(" && ")
            .to_string(),
    );
    assert!(mount_line("/scratch").is_none());
    println!("STACK    a fifth mount on one path was refused");

    // A caller trickling its request one byte at a time cannot hold the broker: every caller
    // gets the whole request read within one bound, so the next caller is answered promptly.
    let mut trickle = spawn_image(
        "exec python3 -c \"import socket, time; s = socket.socket(socket.AF_UNIX); s.connect('/run/alien/mount.sock'); [ (s.send(b' '), time.sleep(1.5)) for _ in range(20) ]\"",
    );
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    refused(
        "/opt/alien/bin/alien-mount tmpfs /workspace -o size=1m",
        "EPERM",
    );
    let took = started.elapsed();
    assert!(
        took < Duration::from_secs(4),
        "a trickling caller held the broker for {took:?}"
    );
    let _ = trickle.kill();
    let _ = trickle.wait();
    println!("TRICKLE  a caller sending a byte every 1.5s did not hold the broker ({took:?})");

    // A pause detach while an NFS request is in flight ends that request first: the server
    // answers after the detach, and its mount must not land then.
    let mut pending = spawn_image(&format!(
        "exec /opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {}",
        options.replace("port=20123", "port=20126")
    ));
    std::thread::sleep(Duration::from_millis(500));
    let started = Instant::now();
    mount_broker::detach_nfs_mounts().expect("detach while a request is in flight");
    let took = started.elapsed();
    let status = pending.wait().unwrap();
    let ended = started.elapsed();
    // Past the slow server's answer.
    std::thread::sleep(Duration::from_secs(3));
    assert!(
        mount_line("/workspace").is_none(),
        "an in-flight NFS mount landed after the pause detach: {:?}",
        mount_line("/workspace")
    );
    assert_eq!(
        status.code(),
        Some(32),
        "the in-flight request was not ended"
    );
    assert!(took < Duration::from_secs(4), "the detach waited {took:?}");
    assert!(
        ended < Duration::from_secs(1),
        "the in-flight request ran on for {ended:?}"
    );
    println!(
        "PAUSE    a detach during an in-flight NFS request ended it ({took:?}); nothing landed"
    );

    // Two NFS mounts stacked on one path both go at pause.
    allowed(&format!(
        "/opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options} && /opt/alien/bin/mount.nfs4 127.0.0.1:/ /workspace -o {options}"
    ));
    let stacked = std::fs::read_to_string("/proc/self/mountinfo")
        .unwrap()
        .lines()
        .filter(|line| line.split(' ').nth(4) == Some("/workspace"))
        .count();
    assert_eq!(stacked, 2, "two NFS mounts on /workspace");
    mount_broker::detach_nfs_mounts().expect("detach");
    assert!(
        mount_line("/workspace").is_none(),
        "a stacked NFS mount survived the pause detach: {:?}",
        mount_line("/workspace")
    );
    println!("PAUSE    both NFS mounts stacked on /workspace were detached");

    for child in [&mut proxy, &mut foreign, &mut hung, &mut slow] {
        let _ = child.kill();
        let _ = child.wait();
    }

    provider_attribution();
}

/// With image permissions a local caller whose socket carries no mark counts as the provider
/// only if its socket's cgroup is outside the guest's. The provider's processes are outside it
/// (on the provider, even outside the VM's view); the image's processes cannot leave it. Every
/// hook and control route goes through this gate (`authorize_control` answers 403 when it says
/// no).
fn provider_attribution() {
    // The agent itself runs in the guest cgroup; the callers run outside it, in the root cgroup,
    // as provider processes and the kernel's helpers do. No socket marker is attached here, so
    // every socket carries mark 0.
    std::fs::write(
        "/sys/fs/cgroup/alien-guest/cgroup.procs",
        std::process::id().to_string(),
    )
    .expect("join the guest cgroup");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    // `root`: outside the guest cgroup, capabilities or none. `image`: in the image cgroup.
    for (who, cgroup, capabilities, expected) in [
        ("provider", "root", true, true),
        ("capless root-cgroup", "root", false, true),
        ("image", "image", true, false),
    ] {
        let mut caller = Command::new("python3");
        caller.args([
            "-c",
            "import socket, sys, time; s = socket.create_connection(('127.0.0.1', int(sys.argv[1]))); time.sleep(30)",
            &port,
        ]);
        let in_image = cgroup == "image";
        // SAFETY: open, write and capability calls before exec; nothing allocates.
        unsafe {
            caller.pre_exec(move || {
                if in_image {
                    mount_broker::join_image_cgroup()?;
                } else {
                    let procs = libc::open(c"/sys/fs/cgroup/cgroup.procs".as_ptr(), libc::O_WRONLY);
                    if procs < 0 || libc::write(procs, b"0".as_ptr().cast(), 1) != 1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    libc::close(procs);
                }
                if !capabilities {
                    privilege::restrict_capabilities(0)?;
                }
                Ok(())
            });
        }
        let mut caller = caller.spawn().unwrap();
        let (_connection, peer) = listener.accept().unwrap();
        let served = runtime.block_on(crate::peer::image_transport_may_serve(peer));
        let _ = caller.kill();
        let _ = caller.wait();
        println!("PROVIDER mark-0 {who} caller served: {served}");
        assert_eq!(served, expected, "{who}");
    }
}
