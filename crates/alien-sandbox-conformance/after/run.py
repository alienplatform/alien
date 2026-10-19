"""After-only live SDK probes. Never use this runner for a before measurement."""

import argparse
import asyncio
import hashlib
import ipaddress
import json
import os
import re
import sys
import time
import uuid
from pathlib import Path
from urllib.parse import urlsplit

GUEST = (Path(__file__).parents[1] / "src/probe.py").read_text()


def now():
    return time.time()


def http(url):
    return {
        "kind": "http",
        "url": url,
        "useProxy": True,
        "serverName": None,
        "hostHeader": None,
    }


def response(value, digest):
    return (
        value.get("outcome") == "response"
        and 200 <= value["status"] < 300
        and value["bodySha256"] == digest
    )


def denied(value):
    return value == {"outcome": "proxyDenied", "status": 403}


def verdict(control_ok, subject_ok, reachable=False):
    if not control_ok:
        return "unproven"
    return "pass" if subject_ok else "fail" if reachable else "unproven"


def reachable(value):
    return value.get("outcome") in ("response", "connected")


def validate(suite):
    if suite["mode"] != "after" or not suite["revision"]:
        raise ValueError("explicit after mode and deployed revision required")
    if suite.get("withoutConnectorBinding") == suite["binding"]:
        raise ValueError("connector refusal needs a distinct fixture binding")
    if "withoutVnetSandboxId" in suite:
        sandbox_id = suite["withoutVnetSandboxId"]
        if (
            not isinstance(sandbox_id, str)
            or not sandbox_id.strip()
            or sandbox_id.strip() != sandbox_id
        ):
            raise ValueError(
                "withoutVnetSandboxId must identify an existing isolated fixture"
            )
    for key in ("allowed", "removed"):
        endpoint = suite[key]
        url = urlsplit(endpoint["url"])
        if url.scheme != "https" or not url.hostname or url.username or url.password:
            raise ValueError("controlled endpoints require credential-free HTTPS URLs")
        if not re.fullmatch(r"[0-9a-f]{64}", endpoint["bodySha256"]):
            raise ValueError("supply the fixture's independently known body digest")
    if (
        urlsplit(suite["allowed"]["url"]).hostname
        == urlsplit(suite["removed"]["url"]).hostname
    ):
        raise ValueError("allowed and removed controls must have different DNS names")
    for target in suite["floor"]:
        if target["category"] not in ("vpc", "overlay", "private", "metadata"):
            raise ValueError("unknown floor category")
        host = urlsplit(target["url"]).hostname
        if urlsplit(target["url"]).scheme != "https" or not host:
            raise ValueError(
                "floor probes require HTTPS to distinguish CONNECT refusal"
            )
        if target["addressForm"] not in ("name", "ip"):
            raise ValueError("floor addressForm must be name or ip")
        if target["addressForm"] == "ip":
            ipaddress.ip_address(host)
        else:
            try:
                ipaddress.ip_address(host)
            except ValueError:
                pass
            else:
                raise ValueError("name probe must use DNS")
    required = {
        (kind, form)
        for kind in ("vpc", "overlay", "private", "metadata")
        for form in ("name", "ip")
    }
    supplied = [(item["category"], item["addressForm"]) for item in suite["floor"]]
    if not set(supplied) <= required or len(supplied) != len(set(supplied)):
        raise ValueError(
            "supply at most one probe for each floor category and address form"
        )
    for address in suite["natIps"]:
        ipaddress.ip_address(address)
    if not suite["natIps"] or urlsplit(suite["ipEchoUrl"]).scheme != "https":
        raise ValueError(
            "independently read NAT IPs and HTTPS plain-text IP echo required"
        )


async def probe(box, sandbox_id, request):
    started = now()
    stream = await box.run_command(
        sandbox_id, "python3", ["-c", GUEST, json.dumps(request)], timeout_ms=30000
    )
    output = bytearray()
    terminal = None
    try:
        async for frame in stream:
            if terminal is not None:
                raise RuntimeError("frame after terminal result")
            if frame.kind == "stdout":
                output.extend(frame.data or b"")
                if len(output) > 8192:
                    raise RuntimeError("probe output exceeded bound")
            elif frame.kind == "exit":
                terminal = (frame.exit_code, frame.truncated)
        if terminal != (0, False):
            raise RuntimeError("probe command failed or was truncated")
        observation = json.loads(output)
    finally:
        await stream.close()
    return {
        "sandboxId": sandbox_id,
        "probe": request,
        "startedAt": started,
        "finishedAt": now(),
        "observation": observation,
    }


async def observe(box, sandbox_id, request):
    return await asyncio.wait_for(probe(box, sandbox_id, request), 45)


async def fingerprint(command, sandbox_id):
    """Trusted host observer emits SHA-256 only; never ask the guest for its token."""
    if not command:
        return None
    argv = [arg.replace("{sandboxId}", sandbox_id) for arg in command]
    proc = await asyncio.create_subprocess_exec(
        *argv, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL
    )
    try:
        try:
            await asyncio.wait_for(proc.stdout.readexactly(66), 10)
        except asyncio.IncompleteReadError as end:
            output = end.partial
        else:
            raise RuntimeError("identity observer exceeded fingerprint output bound")
        await asyncio.wait_for(proc.wait(), 10)
        value = output.decode().strip()
        if proc.returncode or not re.fullmatch(r"[0-9a-f]{64}", value):
            raise RuntimeError("identity observer must emit one SHA-256 fingerprint")
        return {"sandboxId": sandbox_id, "sha256": value, "observedAt": now()}
    finally:
        if proc.returncode is None:
            proc.kill()
            await proc.wait()


async def sample_identity(command, sandbox, endpoint):
    if not command:
        return None, []
    observer = asyncio.create_task(fingerprint(command, sandbox[1]))
    probes = []
    try:
        # Repeated real requests also accommodate remote observer startup. The observer
        # has its own deadline; capture failure leaves this claim unproven.
        while not observer.done():
            probes.append(
                await asyncio.wait_for(observe(*sandbox, http(endpoint["url"])), 10)
            )
            await asyncio.sleep(0.1)
        result = await observer
        if not all(
            response(item["observation"], endpoint["bodySha256"]) for item in probes
        ):
            return None, probes
        return result, probes
    except (OSError, RuntimeError, TimeoutError, ValueError):
        return None, probes
    finally:
        if not observer.done():
            observer.cancel()
        await asyncio.gather(observer, return_exceptions=True)


class Run:
    def __init__(self, suite, journal, factory):
        self.suite = suite
        self.journal = journal
        self.factory = factory
        self.sessions = []
        self.intents = {}
        self.results = []

    def persist(self, record):
        self.journal.write(json.dumps(record) + "\n")
        self.journal.flush()
        os.fsync(self.journal.fileno())

    async def create(self, binding, policy):
        box = self.factory(binding)
        attempt = uuid.uuid4().hex
        intent = {
            "event": "createRequested",
            "binding": binding,
            "attemptId": attempt,
            "startedAt": now(),
        }
        self.intents[attempt] = intent
        self.persist(intent)
        # AWS assigns the ID and intentionally ignores a caller-selected sandbox_id.
        info = await box.create(egress=policy, timeout_ms=600000)
        record = {
            "event": "created",
            "binding": binding,
            "attemptId": attempt,
            "sandboxId": info.sandbox_id,
        }
        # Retain the provider handle before a possible journal write failure.
        self.sessions.append((box, record))
        self.intents.pop(attempt)
        self.persist(record)
        return box, info.sandbox_id

    async def paired(self, name, control, subject, endpoint, expect_denied=False):
        positive = await observe(*control, http(endpoint["url"]))
        actual = await observe(*subject, http(endpoint["url"]))
        healthy = response(positive["observation"], endpoint["bodySha256"])
        ok = (
            denied(actual["observation"])
            if expect_denied
            else response(actual["observation"], endpoint["bodySha256"])
        )
        result = {
            "name": name,
            "control": positive,
            "subject": actual,
            "verdict": verdict(healthy, ok, reachable(actual["observation"])),
        }
        self.results.append(result)
        return result

    async def execute(self):
        suite = self.suite
        a, b = suite["allowed"], suite["removed"]
        targets = [urlsplit(item["url"]).hostname for item in (a, b)]
        control = await self.create(suite["binding"], "internet")
        limited = await self.create(suite["binding"], {"allow": targets[:1]})
        await self.paired("create-listed-name", control, limited, a)
        await self.paired("create-unlisted-name", control, limited, b, True)
        try:
            empty = await self.create(suite["binding"], {"allow": []})
        except Exception as error:  # noqa: BLE001 - retain failed case and run independent probes
            # A create refusal proves nothing about the empty policy's network behavior.
            # Keep its unresolved intent for provider cleanup auditing, and never log raw errors.
            for label in ("allowed", "removed"):
                self.results.append(
                    {
                        "name": "empty-allow-" + label,
                        "verdict": "unproven",
                        "reason": "empty-policy sandbox create failed",
                        "errorCode": getattr(error, "code", type(error).__name__),
                    }
                )
        else:
            for label, endpoint in (("allowed", a), ("removed", b)):
                await self.paired(
                    "empty-allow-" + label, control, empty, endpoint, True
                )
        changing = await self.create(suite["binding"], {"allow": targets})
        before_a = await self.paired("before-update-retained", control, changing, a)
        before_b = await self.paired("before-update-removed", control, changing, b)
        before_token, before_witness = await sample_identity(
            suite.get("identityCommand"), changing, a
        )
        start = time.monotonic()
        started_at = now()
        update_timed_out = False
        try:
            await asyncio.wait_for(
                changing[0].update_egress(changing[1], {"allow": targets[:1]}), 10
            )
        except TimeoutError:
            update_timed_out = True
        # Include API latency and cold command dispatch in the ten-second budget.
        attempts = []
        while not update_timed_out and time.monotonic() - start < 10:
            try:
                actual = await asyncio.wait_for(
                    observe(*changing, http(b["url"])), 10 - (time.monotonic() - start)
                )
            except TimeoutError:
                break
            attempts.append(actual)
            if denied(actual["observation"]):
                break
            await asyncio.sleep(min(0.1, max(0, 10 - (time.monotonic() - start))))
        elapsed = time.monotonic() - start
        after_token, after_witness = await sample_identity(
            suite.get("identityCommand"), changing, a
        )
        retained = await self.paired("after-update-retained", control, changing, a)
        healthy = all(
            item["verdict"] == "pass" for item in (before_a, before_b, retained)
        )
        timely = (
            bool(attempts) and denied(attempts[-1]["observation"]) and elapsed <= 10
        )
        self.results.append(
            {
                "name": "update-within-10s",
                "startedAt": started_at,
                "elapsedSeconds": elapsed,
                "updateTimedOut": update_timed_out,
                "attempts": attempts,
                "verdict": "unproven" if not healthy else "pass" if timely else "fail",
            }
        )
        same = (
            before_token
            and after_token
            and before_token["sha256"] == after_token["sha256"]
        )
        self.results.append(
            {
                "name": "update-without-new-token",
                "before": before_token,
                "after": after_token,
                "witnessProbes": before_witness + after_witness,
                "verdict": "unproven"
                if not (healthy and timely and before_token and after_token)
                else "pass"
                if same
                else "fail",
            }
        )
        supplied = {
            (target["category"], target["addressForm"]) for target in suite["floor"]
        }
        for category in ("vpc", "overlay", "private", "metadata"):
            for form in ("name", "ip"):
                if (category, form) not in supplied:
                    self.results.append(
                        {
                            "name": "floor-" + category + "-" + form,
                            "verdict": "unproven",
                            "reason": "fixture not supplied",
                        }
                    )
        for target in suite["floor"]:
            positive = await observe(*control, http(a["url"]))
            actual = await observe(*control, http(target["url"]))
            self.results.append(
                {
                    "name": "floor-" + target["category"] + "-" + target["addressForm"],
                    "control": positive,
                    "subject": actual,
                    "verdict": verdict(
                        response(positive["observation"], a["bodySha256"]),
                        denied(actual["observation"]),
                        reachable(actual["observation"]),
                    ),
                }
            )
        positive = await observe(*control, http(a["url"]))
        actual = await observe(*control, http(suite["ipEchoUrl"]))
        digests = {
            hashlib.sha256((ip + suffix).encode()).hexdigest()
            for ip in suite["natIps"]
            for suffix in ("", "\n")
        }
        value = actual["observation"]
        matched = any(response(value, digest) for digest in digests)
        self.results.append(
            {
                "name": "egress-ip-equals-nat",
                "control": positive,
                "subject": actual,
                "natIps": suite["natIps"],
                "verdict": verdict(
                    response(positive["observation"], a["bodySha256"]),
                    matched,
                    reachable(value),
                ),
            }
        )
        await self.no_connector(control)
        if "withoutVnetSandboxId" in suite:
            await self.no_vnet(control)

    async def no_vnet(self, control):
        # Azure has no agent. This readback guard is distinct from the agent self-check.
        sandbox_id = self.suite["withoutVnetSandboxId"]
        if sandbox_id == control[1]:
            raise ValueError(
                "missing-VNet fixture must differ from the attached control"
            )
        positive = await observe(*control, http(self.suite["allowed"]["url"]))
        result = {
            "name": "reconnect-without-vnet",
            "control": positive,
            "fixtureSandboxId": sandbox_id,
            "verdict": "unproven",
        }
        self.results.append(result)
        healthy = response(positive["observation"], self.suite["allowed"]["bodySha256"])
        try:
            attached = await asyncio.wait_for(control[0].get(control[1]), 45)
        except Exception as error:  # noqa: BLE001 - retain typed diagnostic, never provider text
            result["controlErrorCode"] = getattr(error, "code", None)
            return
        if not healthy or attached is None or attached.state != "running":
            return
        try:
            found = await asyncio.wait_for(control[0].get(sandbox_id), 45)
        except Exception as error:  # noqa: BLE001 - other refusals cannot prove this guard
            code = getattr(error, "code", None)
            context = getattr(error, "context", None)
            result["errorCode"] = code
            if (
                code == "SANDBOX_NOT_AS_DECLARED"
                and isinstance(context, dict)
                and context.get("sandbox_id") == sandbox_id
                and context.get("restriction") == "deployment network"
            ):
                result["verdict"] = "pass"
        else:
            # A missing/terminated fixture has no running attachment to judge.
            if found is not None and found.state == "running":
                result["verdict"] = "fail"

    async def no_connector(self, control):
        positive = await observe(*control, http(self.suite["allowed"]["url"]))
        healthy = response(positive["observation"], self.suite["allowed"]["bodySha256"])
        binding = self.suite.get("withoutConnectorBinding")
        result = {
            "name": "start-without-connector",
            "control": positive,
            "verdict": "unproven",
        }
        if binding:
            try:
                subject = await self.create(binding, "internet")
                await observe(*subject, http(self.suite["allowed"]["url"]))
                result["verdict"] = "fail" if healthy else "unproven"
            except Exception as error:  # noqa: BLE001 - retain evidence and attempt all cleanup
                # Missing bindings, provisioning timeouts and generic 503s are not self-check proof.
                code = getattr(error, "code", None)
                result["errorCode"] = code
                if code == "EGRESS_PATH_UNEXPECTED" and healthy:
                    result["verdict"] = "pass"
        self.results.append(result)

    async def cleanup(self):
        outcomes = []
        for box, record in reversed(self.sessions):
            try:
                # A create intent may have failed before the session existed.
                info = await box.get(record["sandboxId"])
                if info is not None and info.state != "terminated":
                    await box.terminate(record["sandboxId"])
                outcomes.append({**record, "terminationCallCompleted": True})
            except Exception as error:  # noqa: BLE001 - retain evidence and attempt all cleanup
                outcomes.append(
                    {
                        **record,
                        "terminationCallCompleted": False,
                        "errorCode": getattr(error, "code", type(error).__name__),
                    }
                )
        return outcomes


async def run(suite, directory):
    from alienplatform import Sandbox

    validate(suite)
    directory.mkdir(parents=True, exist_ok=False)
    os.chmod(directory, 0o700)
    report = {"mode": "after", "revision": suite["revision"], "startedAt": now()}
    with (directory / "sessions.jsonl").open("x") as journal:
        runner = Run(suite, journal, Sandbox)
        try:
            await runner.execute()
        except Exception as error:  # noqa: BLE001 - retain evidence and attempt all cleanup
            # Provider diagnostics may carry credentials; only record the typed code.
            report["errorCode"] = getattr(error, "code", type(error).__name__)
        finally:
            report["cleanup"] = await runner.cleanup()
            report["unresolvedCreateIntents"] = list(runner.intents.values())
            report["cases"] = runner.results
            report["finishedAt"] = now()
            report["gatewayLogs"] = (
                "unproven: run verify_logs.py with exported gateway JSON logs"
            )
            (directory / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(directory / "report.json")
    return (
        0
        if (
            not report.get("errorCode")
            and not report["unresolvedCreateIntents"]
            and runner.results
            and all(row["verdict"] == "pass" for row in runner.results)
            and all(row["terminationCallCompleted"] for row in report["cleanup"])
        )
        else 1
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("suite", type=Path)
    parser.add_argument(
        "output", type=Path, help="new directory; never overwrite evidence"
    )
    args = parser.parse_args()
    return asyncio.run(run(json.loads(args.suite.read_text()), args.output))


if __name__ == "__main__":
    sys.exit(main())
