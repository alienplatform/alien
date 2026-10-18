"""Match each successful/denied guest connection to a distinct gateway JSON event."""

import argparse
import json
import sys
from datetime import datetime
from pathlib import Path
from urllib.parse import urlsplit


def observations(report):
    for case in report["cases"]:
        for key in ("control", "subject"):
            if key in case:
                yield case["name"], case[key]
        for attempt in case.get("attempts", []) + case.get("witnessProbes", []):
            yield case["name"], attempt


def verify(report, events, audience):
    checks = []
    candidates = []
    for name, item in observations(report):
        candidates.append([])
        value = item["observation"]
        if value["outcome"] not in ("response", "proxyDenied"):
            checks.append(
                {
                    "case": name,
                    "verdict": "unproven",
                    "reason": "no completed connection",
                }
            )
            continue
        destination = urlsplit(item["probe"]["url"])
        decision = "deny" if value["outcome"] == "proxyDenied" else "allow"
        for index, event in enumerate(events):
            fields = event.get("fields", event)
            if fields.get("message") != "egress connection":
                continue
            if (
                fields.get("aud"),
                fields.get("sub"),
                fields.get("host"),
                fields.get("port"),
                fields.get("decision"),
            ) != (
                audience,
                item["sandboxId"],
                destination.hostname,
                destination.port or 443,
                decision,
            ):
                continue
            stamp = datetime.fromisoformat(fields["ts"]).timestamp()
            # Allow one second of host clock skew and the completion log flush.
            if not item["startedAt"] - 1 <= stamp <= item["finishedAt"] + 1:
                continue
            if not all(
                isinstance(fields.get(key), (int, float)) and fields[key] >= 0
                for key in ("bytesUp", "bytesDown", "durationMs")
            ):
                continue
            # A floor denial must come from the address check, not policy or storage failure.
            if (
                name.startswith("floor-")
                and decision == "deny"
                and fields.get("reason") != "Egress denied: address is not public"
            ):
                continue
            candidates[-1].append(index)
        checks.append(
            {
                "case": name,
                "sandboxId": item["sandboxId"],
                "host": destination.hostname,
                "decision": decision,
                "verdict": "unproven",
            }
        )
    # Clock-skew windows overlap, and per-node exports need not be chronological.
    # Reassign earlier matches when needed so each connection gets a distinct event.
    assigned = {}

    def assign(connection, seen):
        for event in candidates[connection]:
            if event in seen:
                continue
            seen.add(event)
            if event not in assigned or assign(assigned[event], seen):
                assigned[event] = connection
                return True
        return False

    for connection in range(len(checks)):
        assign(connection, set())
    for connection in assigned.values():
        checks[connection]["verdict"] = "pass"
    complete = bool(checks) and all(row["verdict"] == "pass" for row in checks)
    return {
        "revision": report["revision"],
        "audience": audience,
        "connections": checks,
        "verdict": "pass" if complete else "unproven",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument(
        "logs", type=Path, help="JSON lines, one original gateway event per line"
    )
    parser.add_argument("--audience", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    events = [
        json.loads(line) for line in args.logs.read_text().splitlines() if line.strip()
    ]
    result = verify(json.loads(args.report.read_text()), events, args.audience)
    with args.output.open("x") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    return 0 if result["verdict"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
