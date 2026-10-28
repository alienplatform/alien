#!/usr/bin/env python3
"""Stateful Docker CLI fixture; never contacts a daemon or changes its configuration."""
import contextlib
import fcntl
import json
import pathlib
import sys
import time

root = pathlib.Path(__file__).resolve().parent


@contextlib.contextmanager
def state():
    with (root / "lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        value = json.loads((root / "state.json").read_text())
        yield value
        (root / "state.json").write_text(json.dumps(value))


arguments = sys.argv[1:]
with state() as current:
    current["calls"].append(arguments)
    scenario = current["scenario"]

if arguments == ["buildx", "inspect"]:
    if scenario.startswith("provider"):
        print("Name: provider-owned\nDriver: docker-container")
    else:
        endpoint = "selected-context" if scenario == "context" else "default"
        print(f"Name: {endpoint}\nDriver: docker\nEndpoint: {endpoint}")
elif "info" in arguments:
    expected = ["info", "--format", "{{.ID}}"]
    if scenario == "context":
        expected = ["--context", "selected-context", *expected]
    if arguments != expected:
        sys.exit("daemon query did not use the selected endpoint")
    print("synthetic-daemon")
elif arguments[:2] == ["buildx", "create"]:
    if arguments[2:3] != ["--name"] or arguments[4:6] != ["--driver", "docker-container"]:
        sys.exit("unexpected builder creation command")
    endpoint = arguments[6:] or ["default"]
    expected = "selected-context" if scenario == "context" else "default"
    if endpoint != [expected]:
        sys.exit("builder creation did not use the selected endpoint")
    name = arguments[3]
    with state() as current:
        success = name not in current["builders"]
        current["creates"].append({"name": name, "endpoint": expected, "success": success})
        if success:
            current["builders"].append(name)
    sys.exit(0 if success else 1)
elif arguments[:3] == ["buildx", "inspect", "--bootstrap"] and len(arguments) == 4:
    with state() as current:
        if arguments[3] not in current["builders"]:
            sys.exit("cannot bootstrap an unknown builder")
        current["bootstraps"] += 1
        (root / "worker").touch()
    version = "v0.12.5" if scenario == "provider-old" else "v0.13.0"
    print(f"Nodes:\nName: node0\nStatus: running\nBuildKit version: {version}")
elif arguments[:2] == ["buildx", "inspect"] and len(arguments) == 3:
    with state() as current:
        exists = arguments[2] in current["builders"]
        concurrent_miss = scenario == "concurrent" and not exists
        if concurrent_miss:
            current["initialMisses"] += 1
    if concurrent_miss:
        # Both first inspections must miss before either process may create.
        # A deadline makes missing concurrency fail rather than hang the suite.
        deadline = time.monotonic() + 10
        while True:
            with state() as current:
                ready = current["initialMisses"] >= 2
            if ready:
                break
            if time.monotonic() > deadline:
                sys.exit("concurrent first-user rendezvous timed out")
            time.sleep(0.01)
        sys.exit(1)
    sys.exit(0 if exists else 1)
else:
    sys.exit(f"unexpected Docker command: {arguments!r}")
