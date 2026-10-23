"""An OCI boot process that observes its real identity and privileged operations."""
import ctypes
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import sys

from metadata import collect
import gateway

ROOT = Path("/tmp/alien-entrypoint")


def identity():
    fields = dict(line.split(":", 1) for line in Path("/proc/self/status").read_text().splitlines())
    # gVisor omits NoNewPrivs from procfs, but implements the authoritative prctl.
    libc = ctypes.CDLL(None, use_errno=True)
    no_new_privs = libc.prctl(39, 0, 0, 0, 0)  # PR_GET_NO_NEW_PRIVS
    if no_new_privs < 0:
        raise OSError(ctypes.get_errno(), "cannot read no_new_privs")
    return {
        "uid": os.getuid(),
        "gid": os.getgid(),
        "pid": os.getpid(),
        "capabilities": {key: int(fields[key].strip(), 16) for key in
                         ("CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb")},
        "noNewPrivs": no_new_privs,
    }


def mount_probe():
    target = ROOT / "mount"
    target.mkdir(exist_ok=True)
    libc = ctypes.CDLL(None, use_errno=True)
    result = libc.mount(b"tmpfs", os.fsencode(target), b"tmpfs", 0, None)
    error = ctypes.get_errno() if result else 0
    if result == 0 and libc.umount(os.fsencode(target)) != 0:
        raise OSError(ctypes.get_errno(), "could not undo test mount")
    return {"allowed": result == 0, "errno": error}


def set_loopback_mtu(mtu):
    # RTM_NEWLINK is capability-checked on gVisor too; SIOCSIFMTU is unsupported there.
    with socket.socket(socket.AF_NETLINK, socket.SOCK_RAW, socket.NETLINK_ROUTE) as sock:
        sock.settimeout(2)
        info = struct.pack("BxHiII", socket.AF_UNSPEC, 0, socket.if_nametoindex("lo"), 0, 0)
        attribute = struct.pack("HHI", 8, 4, mtu)  # IFLA_MTU
        header = struct.pack("IHHII", 16 + len(info) + len(attribute), 16, 5, 1, 0)
        sock.send(header + info + attribute)  # NLM_F_REQUEST | NLM_F_ACK
        reply = sock.recv(4096)
        assert len(reply) >= 20, "truncated netlink acknowledgement"
        length, kind, flags, sequence, pid = struct.unpack_from("IHHII", reply)
        assert kind == 2 and sequence == 1, "unexpected netlink acknowledgement"
        error = struct.unpack_from("i", reply, 16)[0]
        if error:
            raise OSError(-error, "loopback MTU change refused")


def net_admin_probe():
    # An existing interface avoids confusing an unsupported device type with EPERM.
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        interface = struct.pack("16sI20x", b"lo", 0)
        previous = fcntl.ioctl(sock, 0x8921, interface)  # SIOCGIFMTU
        old_mtu = struct.unpack("16sI20x", previous)[1]
    try:
        set_loopback_mtu(old_mtu - 1)
    except OSError as error:
        return {"allowed": False, "errno": error.errno}
    set_loopback_mtu(old_mtu)
    return {"allowed": True, "errno": 0}


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "identity":
        report = identity()
        if os.environ.get("ALIEN_CONFORMANCE_METADATA"):
            report["metadata"] = collect(os.environ["ALIEN_CONFORMANCE_METADATA"])
        if os.environ.get("ALIEN_CONFORMANCE_GATEWAY"):
            report["gateway"] = gateway.collect(gateway.configuration())
        print(json.dumps(report))
        return
    report = identity()
    if os.environ.get("ALIEN_CONFORMANCE_METADATA"):
        report["metadata"] = collect(os.environ["ALIEN_CONFORMANCE_METADATA"])
    if os.environ.get("ALIEN_CONFORMANCE_GATEWAY"):
        report["gateway"] = gateway.collect(gateway.configuration())
    report["mount"] = mount_probe()
    report["netAdmin"] = net_admin_probe()
    report["child"] = None
    if os.getuid() == 0:
        child = subprocess.run(
            ["setpriv", "--reuid", "2000", "--regid", "2000", "--clear-groups",
             "python3", __file__, "identity"],
            check=True, capture_output=True, text=True,
        )
        report["child"] = json.loads(child.stdout)
    with (ROOT / "starts.jsonl").open("a") as output:
        output.write(json.dumps(report) + "\n")
        output.flush()
        os.fsync(output.fileno())
    (ROOT / "complete").touch()
    while True:
        signal.pause()


if __name__ == "__main__":
    main()
