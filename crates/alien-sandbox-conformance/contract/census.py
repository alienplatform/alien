"""List every process with its uid and capabilities. Run as a job: no secrets, one JSON line."""
import json
from pathlib import Path


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
                "argv": [part.decode(errors="replace") for part in argv if part][:4],
            })
        except (FileNotFoundError, ProcessLookupError):
            pass  # The process exited during enumeration.
    print(json.dumps({"processes": census}))


if __name__ == "__main__":
    main()
