"""List every process with its uid and capabilities. Run as a job: no secrets, one JSON line."""
import ctypes
import json
import os
from pathlib import Path

PR_GET_NO_NEW_PRIVS = 39


def own_no_new_privs():
    """gVisor's /proc status has no NoNewPrivs line, but prctl answers for this process."""
    return int(ctypes.CDLL(None, use_errno=True).prctl(PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0))


def main():
    census = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            fields = dict(line.split(":", 1) for line in (entry / "status").read_text().splitlines())
            argv = (entry / "cmdline").read_bytes().split(b"\0")
            census.append({
                "pid": int(entry.name),
                "uid": int(fields["Uid"].split()[0]),
                "capEff": int(fields["CapEff"].strip(), 16),
                "noNewPrivs": own_no_new_privs() if int(entry.name) == os.getpid()
                else int(fields.get("NoNewPrivs", "-1").strip()),
                "argv": [part.decode(errors="replace") for part in argv if part][:4],
            })
        except (FileNotFoundError, ProcessLookupError):
            pass  # The process exited during enumeration.
    print(json.dumps({"processes": census}))


if __name__ == "__main__":
    main()
