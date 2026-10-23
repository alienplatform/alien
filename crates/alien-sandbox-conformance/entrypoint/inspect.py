"""Run as the first sandbox exec; fail if the image/exec privilege contract is broken."""
import argparse
import errno
import json
from pathlib import Path
import time

from supervisor import ROOT, identity
from metadata import collect, verify
import gateway


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--image-uid", type=int, required=True)
    parser.add_argument("--exec-uid", type=int, required=True)
    parser.add_argument("--metadata-backend", choices=("aws", "gcp", "azure"))
    parser.add_argument("--gateway", action="store_true")
    args = parser.parse_args()
    command = identity()
    # A spawned interpreter may still be loading modules. Observe the boot process
    # immediately, then wait only for its report, not for application readiness.
    present = []
    for proc in Path("/proc").iterdir():
        if proc.name.isdigit():
            try:
                argv = (proc / "cmdline").read_bytes().split(b"\0")
                if b"/opt/entrypoint-conformance/supervisor.py" in argv:
                    present.append(int(proc.name))
            except FileNotFoundError:
                pass  # A process exiting during enumeration is expected.
    assert present, "image command was not running when the first exec started"
    deadline = time.monotonic() + 90
    while not (ROOT / "complete").exists():
        assert time.monotonic() < deadline, "image supervisor never produced its report"
        time.sleep(0.01)
    reports = [json.loads(line) for line in (ROOT / "starts.jsonl").read_text().splitlines()]
    assert len(reports) == 1, f"expected one image start, got {len(reports)}"
    image = reports[0]
    assert image["pid"] in present, "report is from a different boot process"
    evidence = {"image": image, "exec": command, "supervisorPresentAtFirstExec": True}
    if args.metadata_backend:
        command["metadata"] = collect(args.metadata_backend)
    if args.gateway:
        command["gateway"] = gateway.collect(gateway.configuration())
    print(json.dumps(evidence), flush=True)
    if args.gateway:
        for process in [image, command] + ([image["child"]] if args.image_uid == 0 else []):
            gateway.verify(process["gateway"], gateway.configuration())
    if args.metadata_backend:
        identities = [image, command] + ([image["child"]] if args.image_uid == 0 else [])
        for process in identities:
            assert process["metadata"]["backend"] == args.metadata_backend
            verify(process["metadata"])
    assert image["uid"] == args.image_uid, image
    expected_caps = 0xA80405FB if args.image_uid == 0 else 0
    for key, value in image["capabilities"].items():
        assert value == (0 if key in ("CapInh", "CapAmb") else expected_caps), (key, value)
    if args.image_uid == 0:
        assert image["child"]["uid"] == 2000, image["child"]
    for operation in ("mount", "netAdmin"):
        assert image[operation] == {"allowed": False, "errno": errno.EPERM}, image[operation]
    assert command["uid"] == args.exec_uid and args.exec_uid != 0, command
    assert all(value == 0 for value in command["capabilities"].values()), command
    assert command["noNewPrivs"] == 1, command


if __name__ == "__main__":
    main()
