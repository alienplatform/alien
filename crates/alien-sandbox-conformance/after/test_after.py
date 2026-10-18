"""Exercise the after runner through real guest processes and TLS proxy sockets."""

import asyncio
import hashlib
import os
import socketserver
import ssl
import subprocess
import sys
import tempfile
import threading
import unittest
from datetime import UTC, datetime
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

    async def create(self, *, sandbox_id, egress, timeout_ms):
        if self.name == "without-connector":
            raise Refused()
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
        return self.sessions.get(sandbox_id)

    async def terminate(self, sandbox_id):
        server, thread = self.sessions.pop(sandbox_id)
        await asyncio.to_thread(server.shutdown)
        server.server_close()
        thread.join()


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
                    self.assertEqual(5, len(runner.sessions))
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
                    5, len((root / "sessions.jsonl").read_text().splitlines())
                )

    async def test_fingerprint_rejects_secret_or_failed_observer(self):
        self.assertIsNone(await run.fingerprint(None, "sandbox"))
        for command in (
            [sys.executable, "-c", "print('secret.' * 100)"],
            [sys.executable, "-c", "print('a' * 64); exit(1)"],
        ):
            with self.assertRaises(RuntimeError):
                await run.fingerprint(command, "sandbox")

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
        self.assertEqual(
            "pass",
            verify_logs.verify(report, [event], "deployment/resource")["verdict"],
        )
        for change in (
            {"sub": "other"},
            {"aud": "other"},
            {"reason": "DNS unavailable"},
            {"ts": datetime.fromtimestamp(80, UTC).isoformat()},
        ):
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
