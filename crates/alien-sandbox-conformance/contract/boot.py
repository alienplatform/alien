"""Root ENTRYPOINT: record the boot identity once, then run the workload as uid 1001."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

STATE = Path("/tmp/alien-contract")
WORKLOAD_UID = 1001


def status_fields():
    lines = Path("/proc/self/status").read_text().splitlines()
    return dict(line.split(":", 1) for line in lines)


def identity():
    fields = status_fields()
    return {
        "uid": os.getuid(),
        "pid": os.getpid(),
        "capabilities": {key: int(fields[key].strip(), 16)
                         for key in ("CapInh", "CapPrm", "CapEff", "CapBnd", "CapAmb")},
    }


def main():
    report = identity()
    # A backend may mount a fresh volume over /tmp, so the image's own directory can be hidden.
    STATE.mkdir(exist_ok=True)
    STATE.chmod(0o1777)
    with (STATE / "boots.jsonl").open("a") as output:
        output.write(json.dumps(report) + "\n")
        output.flush()
        os.fsync(output.fileno())
    # Root is only needed to change identity. The server gets no capabilities.
    command = ["python3", "/opt/contract-conformance/server.py"]
    if os.getuid() == 0:
        command = ["setpriv", "--reuid", str(WORKLOAD_UID), "--regid", str(WORKLOAD_UID),
                   "--clear-groups", "--inh-caps=-all", "--bounding-set=-all",
                   "--no-new-privs"] + command
    server = subprocess.Popen(command)
    signal.signal(signal.SIGTERM, lambda *_: server.terminate())
    sys.exit(server.wait())


if __name__ == "__main__":
    main()
