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


def validate_identity_map(identity_map, report, audience):
    if not isinstance(identity_map, dict) or (
        identity_map.get("audience"),
        identity_map.get("revision"),
    ) != (audience, report["revision"]):
        raise ValueError("identity map must match the report revision and audience")
    subjects = identity_map.get("subjects")
    if not isinstance(subjects, dict) or not all(
        isinstance(value, str) and value and value.strip() == value
        for pair in subjects.items()
        for value in pair
    ):
        raise ValueError("identity map subjects must map nonempty IDs to subjects")
    if len(set(subjects.values())) != len(subjects):
        raise ValueError("identity map must not share a subject between provider IDs")
    return subjects


def unique_events(events):
    """Drop repeated copies of one gateway event.

    Overlapping exports can deliver the same record twice, and each copy must not
    prove a separate connection. The gateway emits no event ID, but each record
    carries a nanosecond timestamp, byte counts and a duration, so two distinct
    connections do not produce identical fields.
    """
    seen = set()
    unique = []
    for event in events:
        key = json.dumps(event.get("fields", event), sort_keys=True)
        if key not in seen:
            seen.add(key)
            unique.append(event)
    return unique


def verify(report, events, audience, identity_map=None):
    events = unique_events(events)
    subjects = (
        validate_identity_map(identity_map, report, audience)
        if identity_map is not None
        else None
    )
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
        subject = (
            subjects.get(item["sandboxId"])
            if subjects is not None
            else item["sandboxId"]
        )
        check = {
            "case": name,
            "sandboxId": item["sandboxId"],
            "host": destination.hostname,
            "decision": decision,
            "verdict": "unproven",
        }
        checks.append(check)
        if subjects is not None:
            check["gatewaySubject"] = subject
        if subject is None:
            check["reason"] = "provider identity mapping is missing"
            continue
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
                subject,
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
            # A floor denial must come from the address check, not storage failure. A
            # literal IP never reaches that check: `internet` never authorizes literals,
            # and a policy cannot grant a private range, so the policy refusal is the
            # floor for the IP form.
            floor_reasons = {"Egress denied: address is not public"}
            if name.endswith("-ip"):
                floor_reasons.add("Egress target outside stored policy")
            if (
                name.startswith("floor-")
                and decision == "deny"
                and fields.get("reason") not in floor_reasons
            ):
                continue
            candidates[-1].append(index)
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
    parser.add_argument(
        "--identity-map",
        type=Path,
        help="trusted provider ID to gateway subject mapping for this revision/audience",
    )
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    events = [
        json.loads(line) for line in args.logs.read_text().splitlines() if line.strip()
    ]
    try:
        identity_map = (
            json.loads(args.identity_map.read_text()) if args.identity_map else None
        )
        if args.identity_map and identity_map is None:
            raise ValueError("identity map must be a JSON object")
        result = verify(
            json.loads(args.report.read_text()), events, args.audience, identity_map
        )
    except ValueError as error:
        parser.error(str(error))
    with args.output.open("x") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    return 0 if result["verdict"] == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
