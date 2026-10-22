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

ROOT = Path("/tmp/alien-entrypoint")


def identity():
    fields = dict(line.split(":", 1) for line in Path("/proc/self/status").read_text().splitlines())
    return {
        "uid": os.getuid(),
        "gid": os.getgid(),
        "pid": os.getpid(),
        "capabilities": {key: int(fields[key].strip(), 16) for key in
                         ("CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb")},
        "noNewPrivs": int(fields["NoNewPrivs"]),
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


def net_admin_probe():
    # An existing interface avoids confusing an unsupported device type with EPERM.
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        interface = struct.pack("16sI20x", b"lo", 0)
        previous = fcntl.ioctl(sock, 0x8921, interface)  # SIOCGIFMTU
        old_mtu = struct.unpack("16sI20x", previous)[1]
        change = struct.pack("16sI20x", b"lo", old_mtu - 1)
        try:
            fcntl.ioctl(sock, 0x8922, change)  # SIOCSIFMTU
        except OSError as error:
            return {"allowed": False, "errno": error.errno}
        fcntl.ioctl(sock, 0x8922, previous)
        return {"allowed": True, "errno": 0}


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "identity":
        report = identity()
        if os.environ.get("ALIEN_CONFORMANCE_METADATA"):
            report["metadata"] = collect(os.environ["ALIEN_CONFORMANCE_METADATA"])
        print(json.dumps(report))
        return
    report = identity()
    if os.environ.get("ALIEN_CONFORMANCE_METADATA"):
        report["metadata"] = collect(os.environ["ALIEN_CONFORMANCE_METADATA"])
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
