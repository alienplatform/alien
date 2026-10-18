"""Exercise the after runner through real guest processes and TLS proxy sockets."""

import asyncio
import base64
import hashlib
import json
import os
import socket
import socketserver
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import uuid
from datetime import UTC, datetime
from itertools import permutations
from pathlib import Path
from types import SimpleNamespace

import run
import verify_logs

BODY = b"controlled endpoint\n"
DIGEST = hashlib.sha256(BODY).hexdigest()


class Proxy(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


class Handler(socketserver.StreamRequestHandler):
    def handle(self):
        request = self.rfile.readline().decode().split()
        while self.rfile.readline() != b"\r\n":
            pass
        host = request[1].split(":")[0]
        allowed = host in ("allowed.test", "removed.test", "echo.test") and (
            self.server.policy == "internet" or host in self.server.policy["allow"]
        )
        if not allowed:
            self.wfile.write(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            return
        self.wfile.write(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        self.wfile.flush()
        with self.server.tls.wrap_socket(self.connection, server_side=True) as stream:
            data = b""
            while not data.endswith(b"\r\n\r\n"):
                data += stream.recv(1)
            body = b"203.0.113.4\n" if host == "echo.test" else BODY
            stream.sendall(
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: "
                + str(len(body)).encode()
                + b"\r\n\r\n"
                + body
            )


class Stream:
    def __init__(self, proc):
        self.proc = proc
        self.frames = None

    def __aiter__(self):
        return self

    async def __anext__(self):
        if self.frames is None:
            stdout, _stderr = await self.proc.communicate()
            self.frames = iter(
                [
                    SimpleNamespace(kind="stdout", data=stdout),
                    SimpleNamespace(
                        kind="exit", exit_code=self.proc.returncode, truncated=False
                    ),
                ]
            )
        try:
            return next(self.frames)
        except StopIteration:
            raise StopAsyncIteration from None

    async def close(self):
        if self.proc.returncode is None:
            self.proc.kill()
            await self.proc.wait()


class Refused(Exception):
    code = "EGRESS_PATH_UNEXPECTED"


class Box:
    def __init__(self, name, tls, cert):
        self.name, self.tls, self.cert = name, tls, cert
        self.sessions = {}
        self.terminated = set()

    async def create(self, *, egress, timeout_ms):
        if self.name == "without-connector":
            raise Refused()
        sandbox_id = "provider-" + uuid.uuid4().hex
        server = Proxy(("127.0.0.1", 0), Handler)
        server.policy, server.tls = egress, self.tls
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.sessions[sandbox_id] = server, thread
        return SimpleNamespace(sandbox_id=sandbox_id)

    async def update_egress(self, sandbox_id, egress):
        self.sessions[sandbox_id][0].policy = egress

    async def run_command(self, sandbox_id, command, args, *, timeout_ms):
        server = self.sessions[sandbox_id][0]
        env = dict(
            os.environ,
            SSL_CERT_FILE=str(self.cert),
            HTTPS_PROXY=f"http://127.0.0.1:{server.server_address[1]}",
        )
        proc = await asyncio.create_subprocess_exec(
            command,
            *args,
            env=env,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        return Stream(proc)

    async def get(self, sandbox_id):
        if sandbox_id in self.terminated:
            return SimpleNamespace(state="terminated")
        return SimpleNamespace(state="running") if sandbox_id in self.sessions else None

    async def terminate(self, sandbox_id):
        server, thread = self.sessions.pop(sandbox_id)
        self.terminated.add(sandbox_id)
        await asyncio.to_thread(server.shutdown)
        server.server_close()
        thread.join()


class TimeoutBox(Box):
    async def update_egress(self, sandbox_id, egress):
        raise TimeoutError()


class EmptyRefusedBox(Box):
    async def create(self, *, egress, timeout_ms):
        if egress == {"allow": []}:
            raise Refused("sensitive provider response")
        return await super().create(egress=egress, timeout_ms=timeout_ms)


class After(unittest.IsolatedAsyncioTestCase):
    async def test_live_transport_and_policy_transitions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cert, key = root / "cert.pem", root / "key.pem"
            await asyncio.to_thread(
                subprocess.run,
                [
                    "openssl",
                    "req",
                    "-x509",
                    "-newkey",
                    "rsa:2048",
                    "-nodes",
                    "-keyout",
                    str(key),
                    "-out",
                    str(cert),
                    "-days",
                    "1",
                    "-subj",
                    "/CN=allowed.test",
                    "-addext",
                    "subjectAltName=DNS:allowed.test,DNS:removed.test,DNS:echo.test",
                ],
                check=True,
                capture_output=True,
                timeout=30,
            )
            tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
            tls.load_cert_chain(cert, key)
            suite = {
                "mode": "after",
                "revision": "test",
                "binding": "gateway",
                "allowed": {"url": "https://allowed.test/", "bodySha256": DIGEST},
                "removed": {"url": "https://removed.test/", "bodySha256": DIGEST},
                "floor": [
                    {
                        "category": kind,
                        "addressForm": form,
                        "url": "https://10.244.0.1/"
                        if form == "ip"
                        else "https://private.test/",
                    }
                    for kind in ("vpc", "overlay", "private", "metadata")
                    for form in ("name", "ip")
                ],
                "natIps": ["203.0.113.4"],
                "ipEchoUrl": "https://echo.test/",
                "withoutConnectorBinding": "without-connector",
                "identityCommand": [sys.executable, "-c", "print('a' * 64)"],
            }
            run.validate(suite)
            with (root / "sessions.jsonl").open("x") as journal:
                runner = run.Run(suite, journal, lambda name: Box(name, tls, cert))
                try:
                    await runner.execute()
                    self.assertEqual(19, len(runner.results))
                    self.assertTrue(
                        all(item["verdict"] == "pass" for item in runner.results),
                        runner.results,
                    )
                    self.assertEqual(4, len(runner.sessions))
                    self.assertTrue(
                        all(
                            record["sandboxId"].startswith("provider-")
                            for _, record in runner.sessions
                        )
                    )
                    self.assertEqual(1, len(runner.intents))
                    # A faulty implementation that ignores policy replacement must fail the probe.
                    control, limited = runner.sessions[:2]
                    limited[0].sessions[limited[1]["sandboxId"]][0].policy = "internet"
                    result = await runner.paired(
                        "ignored-policy",
                        (control[0], control[1]["sandboxId"]),
                        (limited[0], limited[1]["sandboxId"]),
                        suite["removed"],
                        True,
                    )
                    self.assertEqual("fail", result["verdict"])
                    # A broken fixture must not turn a denial into a passing result.
                    bad = dict(suite["removed"], bodySha256="b" * 64)
                    result = await runner.paired(
                        "bad-control",
                        (control[0], control[1]["sandboxId"]),
                        (limited[0], limited[1]["sandboxId"]),
                        bad,
                        True,
                    )
                    self.assertEqual("unproven", result["verdict"])
                finally:
                    cleanup = await runner.cleanup()
                self.assertTrue(
                    all(item["terminationCallCompleted"] for item in cleanup)
                )
                self.assertEqual(
                    9, len((root / "sessions.jsonl").read_text().splitlines())
                )
                repeated = await runner.cleanup()
                self.assertTrue(
                    all(item["terminationCallCompleted"] for item in repeated)
                )
            partial = dict(suite, floor=[], identityCommand=None)
            run.validate(partial)
            with (root / "partial.jsonl").open("x") as journal:
                runner = run.Run(
                    partial, journal, lambda name: TimeoutBox(name, tls, cert)
                )
                try:
                    await runner.execute()
                    rows = {item["name"]: item for item in runner.results}
                    self.assertEqual("fail", rows["update-within-10s"]["verdict"])
                    self.assertEqual(
                        "unproven", rows["update-without-new-token"]["verdict"]
                    )
                    self.assertEqual("pass", rows["egress-ip-equals-nat"]["verdict"])
                    floors = [
                        row for name, row in rows.items() if name.startswith("floor-")
                    ]
                    self.assertEqual(8, len(floors))
                    self.assertTrue(all(row["verdict"] == "unproven" for row in floors))
                finally:
                    cleanup = await runner.cleanup()
                self.assertTrue(
                    all(item["terminationCallCompleted"] for item in cleanup)
                )

            with (root / "empty-refused.jsonl").open("x") as journal:
                runner = run.Run(
                    suite, journal, lambda name: EmptyRefusedBox(name, tls, cert)
                )
                try:
                    await runner.execute()
                    self.assertEqual(19, len(runner.results))
                    for row in runner.results:
                        if row["name"].startswith("empty-allow-"):
                            self.assertEqual("unproven", row["verdict"])
                            self.assertEqual("EGRESS_PATH_UNEXPECTED", row["errorCode"])
                        else:
                            self.assertEqual("pass", row["verdict"], row)
                    self.assertEqual(2, len(runner.intents))
                    self.assertEqual(3, len(runner.sessions))
                    self.assertNotIn(
                        "sensitive provider response", json.dumps(runner.results)
                    )
                finally:
                    cleanup = await runner.cleanup()
                self.assertEqual(3, len(cleanup))
                self.assertTrue(
                    all(item["terminationCallCompleted"] for item in cleanup)
                )
                self.assertTrue(all(not box.sessions for box, _ in runner.sessions))

    @unittest.skipUnless(os.geteuid() == 0, "host packet witness requires CAP_NET_RAW")
    async def test_packet_witness_captures_only_selected_identity(self):
        def token(subject):
            claims = json.dumps(
                {"iss": "alien-sandbox", "aud": "test/resource", "sub": subject}
            ).encode()
            return (
                b"header."
                + base64.urlsafe_b64encode(claims).rstrip(b"=")
                + b".test-signature"
            )

        expected = token("selected")
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            listener.listen()
            listener.settimeout(2)
            port = listener.getsockname()[1]
            observer = await asyncio.create_subprocess_exec(
                sys.executable,
                str(Path(__file__).with_name("capture_identity.py")),
                "--audience",
                "test/resource",
                "--sandbox-id",
                "selected",
                "--interface",
                "lo",
                "--port",
                str(port),
                "--timeout",
                "2",
                stdout=asyncio.subprocess.PIPE,
            )
            try:
                deadline = time.monotonic() + 2
                while observer.returncode is None and time.monotonic() < deadline:
                    for identity in (token("wrong-sandbox"), expected):
                        with (
                            socket.create_connection(
                                listener.getsockname(), timeout=2
                            ) as client,
                            listener.accept()[0] as server,
                        ):
                            client.sendall(
                                b"CONNECT allowed.test:443 HTTP/1.1\r\n"
                                b"Proxy-Authorization: Bearer " + identity + b"\r\n\r\n"
                            )
                            server.recv(8192)
                    await asyncio.sleep(0.05)
                output, _ = await asyncio.wait_for(observer.communicate(), 3)
                self.assertEqual(0, observer.returncode)
                self.assertEqual(
                    hashlib.sha256(expected).hexdigest() + "\n", output.decode()
                )
            finally:
                if observer.returncode is None:
                    observer.kill()
                    await observer.wait()

    async def test_fingerprint_rejects_secret_or_failed_observer(self):
        self.assertIsNone(await run.fingerprint(None, "sandbox"))
        for command in (
            [sys.executable, "-c", "print('secret.' * 100)"],
            [sys.executable, "-c", "print('a' * 64); exit(1)"],
        ):
            with self.assertRaises(RuntimeError):
                await run.fingerprint(command, "sandbox")

    def test_connection_log_matching_is_independent_of_input_order(self):
        cases = [
            {
                "name": "connection-" + str(index),
                "subject": {
                    "sandboxId": "sandbox",
                    "startedAt": 100 + index,
                    "finishedAt": 101 + index,
                    "probe": run.http("https://allowed.test/"),
                    "observation": {"outcome": "response"},
                },
            }
            for index in range(3)
        ]
        events = [
            {
                "message": "egress connection",
                "aud": "deployment/resource",
                "sub": "sandbox",
                "host": "allowed.test",
                "port": 443,
                "decision": "allow",
                "bytesUp": 1,
                "bytesDown": 1,
                "durationMs": 1,
                "ts": datetime.fromtimestamp(stamp, UTC).isoformat(),
            }
            for stamp in (100, 101.5, 102.5)
        ]
        # Overlapping clock-skew windows require reassigning an earlier match.
        # Both per-node log exports and report traversal may be out of time order.
        for case_order in permutations(cases):
            report = {"revision": "test", "cases": case_order}
            for event_order in permutations(events):
                self.assertEqual(
                    "pass",
                    verify_logs.verify(report, event_order, "deployment/resource")[
                        "verdict"
                    ],
                )
            self.assertEqual(
                "unproven",
                verify_logs.verify(report, events[:2], "deployment/resource")[
                    "verdict"
                ],
            )

        # A general denial must not consume the only address-floor witness.
        cases = cases[:2]
        cases[1]["name"] = "floor-vpc-name"
        for case in cases:
            case["subject"]["observation"] = {"outcome": "proxyDenied"}
        events = events[1:]
        for event, reason in zip(
            events, ("address is not public", "target is not allowed")
        ):
            event["decision"] = "deny"
            event["reason"] = "Egress denied: " + reason
        # Both observations can see either event, but only one proves the floor.
        cases[0]["subject"]["finishedAt"] = 102
        for event_order in permutations(events):
            self.assertEqual(
                "pass",
                verify_logs.verify(
                    {"revision": "test", "cases": cases},
                    event_order,
                    "deployment/resource",
                )["verdict"],
            )

    def test_connection_log_requires_distinct_scoped_current_events(self):
        observation = {
            "sandboxId": "sandbox",
            "startedAt": 100,
            "finishedAt": 101,
            "probe": run.http("https://private.test/"),
            "observation": {"outcome": "proxyDenied", "status": 403},
        }
        report = {
            "revision": "test",
            "cases": [{"name": "floor-vpc-name", "subject": observation}],
        }
        event = {
            "message": "egress connection",
            "aud": "deployment/resource",
            "sub": "sandbox",
            "host": "private.test",
            "port": 443,
            "decision": "deny",
            "bytesUp": 1,
            "bytesDown": 1,
            "durationMs": 1,
            "reason": "Egress denied: address is not public",
            "ts": datetime.fromtimestamp(100.5, UTC).isoformat(),
        }
        for form, host in (("name", "private.test"), ("ip", "10.42.3.10")):
            report["cases"][0]["name"] = "floor-vpc-" + form
            observation["probe"] = run.http("https://" + host + "/")
            event["host"] = host
            with self.subTest(address_form=form):
                self.assertEqual(
                    "pass",
                    verify_logs.verify(report, [event], "deployment/resource")[
                        "verdict"
                    ],
                )
                for change in (
                    {"sub": "other"},
                    {"aud": "other"},
                    {"reason": "DNS unavailable"},
                    {"reason": "policy store unavailable"},
                    {"reason": "Egress denied: target is not allowed"},
                    {"ts": datetime.fromtimestamp(80, UTC).isoformat()},
                ):
                    with self.subTest(change=change):
                        self.assertEqual(
                            "unproven",
                            verify_logs.verify(
                                report, [dict(event, **change)], "deployment/resource"
                            )["verdict"],
                        )
        report["cases"].append(report["cases"][0])
        self.assertEqual(
            "unproven",
            verify_logs.verify(report, [event], "deployment/resource")["verdict"],
        )


if __name__ == "__main__":
    unittest.main()
