#!/usr/bin/env python3
"""Real proxy conformance and benchmark. Requires Docker and Python cryptography.

Run with --image IMAGE --output DIR. Uses an internal Docker bridge with public
address space solely to exercise public-address checks without internet services.
All containers and the bridge are removed in finally; evidence stays outside git.
"""
import argparse
import base64
import datetime
import http.client
import hashlib
import json
import os
from pathlib import Path
import socket
import ssl
import statistics
import subprocess
import time

from agent_capabilities import AGENT_CAP_ARGS, check_capabilities
from system_egress import check_system_egress

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, ed25519
from cryptography.x509.oid import NameOID

AUD = "test/box"
SUB = "sandbox-one"
ORIGIN = "93.184.216.2"
PROXY = "93.184.216.3"
BODY = b"controlled-origin-response\n"
PREFIX = "alien1609-gw-conformance"
AGENT_UID = 60000


def docker(*args):
    result = subprocess.run(
        ["docker", *map(str, args)],
        env={**os.environ, "DOCKER_HOST": "unix:///var/run/docker.sock"},
        stderr=subprocess.STDOUT, stdout=subprocess.PIPE,
    )
    if result.returncode:
        raise RuntimeError(result.stdout.decode())
    return result.stdout.decode().strip()


def encoded(value):
    return base64.urlsafe_b64encode(value).rstrip(b"=").decode()


def check_loopback_http(container):
    script = '''import http.server,os,subprocess,threading,urllib.request
from pathlib import Path
env=dict(line.split("=",1) for line in Path("/data/image-env").read_text().splitlines())
os.environ.clear();os.environ.update(env)
class Handler(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  self.send_response(200);self.end_headers();self.wfile.write(b"loopback-http-ok")
 def log_message(self,*args): pass
server=http.server.ThreadingHTTPServer(("127.0.0.1",0),Handler)
threading.Thread(target=server.serve_forever,daemon=True).start()
for host in ["localhost","127.0.0.1"]:
 url=f"http://{host}:{server.server_port}/"
 assert urllib.request.urlopen(url,timeout=3).read()==b"loopback-http-ok"
 assert subprocess.check_output(["curl","--fail","--silent","--max-time","3",url])==b"loopback-http-ok"
server.shutdown();print("loopback-http-ok")
'''
    assert docker("exec", "-u", f"{AGENT_UID}:{AGENT_UID}", container, "python", "-c", script) == "loopback-http-ok"



def check_remote_preview(container, uid, port):
    marker = f"/tmp/preview-{port}"
    script = f'''import http.server
from pathlib import Path
class Handler(http.server.BaseHTTPRequestHandler):
 def do_GET(self):
  self.send_response(200);self.end_headers();self.wfile.write(b"remote-preview-ok")
 def log_message(self,*args): pass
server=http.server.HTTPServer(("0.0.0.0",{port}),Handler)
server.timeout=10
Path({marker!r}).touch()
server.handle_request();server.server_close()
'''
    docker("exec", "-d", "-u", uid, container, "python", "-c", script)
    deadline = time.monotonic() + 5
    while docker("exec", container, "sh", "-c", f"test -f {marker} && echo ready || true") != "ready":
        assert time.monotonic() < deadline, "preview server failed to start"
        time.sleep(.05)
    conn = http.client.HTTPConnection("172.29.91.3", port, timeout=3)
    try:
        conn.request("GET", "/")
        response = conn.getresponse()
        assert response.status == 200 and response.read() == b"remote-preview-ok"
    finally:
        conn.close()


def policy_digest(policy):
    if isinstance(policy, dict):
        policy = {"allow": sorted(policy["allow"])}
    return "sha256:" + hashlib.sha256(json.dumps(policy, separators=(",", ":")).encode()).hexdigest()


def certificates(directory):
    now = datetime.datetime.now(datetime.timezone.utc)
    ca_key = ec.generate_private_key(ec.SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Controlled test CA")])
    ca = (x509.CertificateBuilder().subject_name(name).issuer_name(name)
          .public_key(ca_key.public_key()).serial_number(x509.random_serial_number())
          .not_valid_before(now - datetime.timedelta(minutes=1))
          .not_valid_after(now + datetime.timedelta(days=1))
          .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
          .add_extension(x509.SubjectKeyIdentifier.from_public_key(ca_key.public_key()), False)
          .add_extension(x509.KeyUsage(True, False, False, False, False, True, True, False, False), True)
          .sign(ca_key, hashes.SHA256()))
    names = ["allowed.test", "inject.test", "rebind.test", "private.test", "mixed.test", "denied.test", "example.test", "*.example.test"]
    for label in ["origin", "untrusted"]:
        key = ec.generate_private_key(ec.SECP256R1())
        issuer_key = ca_key if label == "origin" else key
        cert = (x509.CertificateBuilder().subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Controlled origin")])).issuer_name(name)
                .public_key(key.public_key()).serial_number(x509.random_serial_number())
                .not_valid_before(now - datetime.timedelta(minutes=1))
                .not_valid_after(now + datetime.timedelta(days=1))
                .add_extension(x509.SubjectAlternativeName([x509.DNSName(n) for n in names]), False)
                .add_extension(x509.AuthorityKeyIdentifier.from_issuer_public_key(issuer_key.public_key()), False)
                .sign(issuer_key, hashes.SHA256()))
        (directory / f"{label}.pem").write_bytes(cert.public_bytes(serialization.Encoding.PEM))
        (directory / f"{label}.key").write_bytes(key.private_bytes(
            serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
    (directory / "ca.pem").write_bytes(ca.public_bytes(serialization.Encoding.PEM))
    return base64.b64encode(ca.public_bytes(serialization.Encoding.DER)).decode()


class Suite:
    def __init__(self, output):
        self.output = output
        self.key = ed25519.Ed25519PrivateKey.generate()
        self.results = []
        self.policy = "internet"

    def token(self, admin=False, **overrides):
        now = int(time.time())
        claims = dict(iss="alien-sandbox", aud=AUD, sub=SUB, iat=now, exp=now + (60 if admin else 3600))
        claims.update(overrides)
        header = dict(alg="EdDSA", kid="test")
        if admin:
            header["typ"] = "alien-egress-admin"
        message = encoded(json.dumps(header).encode()) + "." + encoded(json.dumps(claims).encode())
        return message + "." + encoded(self.key.sign(message.encode()))

    def record(self, name, run):
        start = time.monotonic()
        run()
        result = dict(name=name, verdict="PASS", durationMs=round((time.monotonic() - start) * 1000, 2))
        self.results.append(result)
        (self.output / "results.json").write_text(json.dumps(self.results, indent=2))
        print(json.dumps(result), flush=True)

    def tunnel(self, host="allowed.test", port=443, token=None, expected=200, proxy=PROXY):
        stream = socket.create_connection((proxy, 3128), timeout=8)
        authority = f"{host}:{port}"
        stream.sendall(f"CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\nProxy-Authorization: Bearer {token or self.token()}\r\n\r\n".encode())
        response = b""
        while not response.endswith(b"\r\n\r\n"):
            part = stream.recv(1)
            assert part, response
            response += part
        assert int(response.split()[1]) == expected, response
        if expected != 200:
            stream.close()
        return stream

    def tls(self, host="allowed.test", port=443, sni=None, ca=None, token=None, path="/", method="GET", host_header=None, direct=False):
        stream = socket.create_connection((ORIGIN, port), timeout=8) if direct else self.tunnel(host, port, token)
        context = ssl.create_default_context(cafile=str(ca or self.output / "origin/ca.pem"))
        with context.wrap_socket(stream, server_hostname=sni or host) as tls:
            tls.sendall(f"{method} {path} HTTP/1.1\r\nHost: {host_header or host}\r\nConnection: close\r\n\r\n".encode())
            response = http.client.HTTPResponse(tls)
            response.begin()
            return response.status, response.read()

    def admin(self, path, body, token=None, proxy=PROXY, method=None):
        method = method or ("PUT" if path == "/v1/policies" else "POST")
        claims = {k: v for k, v in body.items() if k in ["aud", "sub"]}
        claims["method"] = method
        if "policy" in body:
            claims["policy"] = policy_digest(body["policy"])
        conn = http.client.HTTPConnection(proxy, 3129, timeout=5)
        conn.request(method, path, json.dumps(body), {"Content-Type": "application/json", "Authorization": "Bearer " + (token or self.token(admin=True, **claims))})
        response = conn.getresponse()
        result = response.status, response.read()
        conn.close()
        return result

    def denied_tls(self, **kwargs):
        try:
            self.tls(**kwargs)
        except (ssl.SSLError, ConnectionError, http.client.RemoteDisconnected):
            return
        raise AssertionError("TLS unexpectedly completed")

    def plain(self, target="allowed.test", host="allowed.test", connect=False):
        stream = self.tunnel(port=80) if connect else socket.create_connection((PROXY, 3128), timeout=8)
        path = "/" if connect else f"http://{target}/"
        stream.sendall(f"GET {path} HTTP/1.1\r\nHost: {host}\r\nProxy-Authorization: Bearer {self.token()}\r\nConnection: close\r\n\r\n".encode())
        response = http.client.HTTPResponse(stream)
        response.begin()
        result = response.status, response.read()
        stream.close()
        return result

    def run(self):
        def equal(actual, expected):
            assert actual == expected, (actual, expected)

        equal(self.admin("/v1/policies", {"aud": AUD, "sub": SUB, "policy": self.policy})[0], 204)
        def health(token):
            conn = http.client.HTTPConnection(PROXY, 3128, timeout=5)
            conn.request("GET", "/health", headers={"Proxy-Authorization":"Bearer " + token})
            response = conn.getresponse()
            result = response.status, response.read()
            conn.close()
            return result
        self.record("authenticated proxy-port health", lambda: equal(health(self.token()), (200, b"ok\n")))
        self.record("health refuses identity without policy despite configured default", lambda: equal(health(self.token(sub="missing"))[0], 403))
        self.record("obsolete generation token refused", lambda: self.tunnel(token=self.token(gen=0), expected=403))
        self.record("direct verified TLS control", lambda: equal(self.tls(direct=True), (200, BODY)))
        self.record("allowed name verified TLS", lambda: equal(self.tls(), (200, BODY)))
        self.record("plain HTTP allowed", lambda: equal(self.plain(), (200, BODY)))
        self.record("wildcard includes apex", lambda: equal(self.tls(host="example.test"), (200, BODY)))
        def proxy_url():
            result = subprocess.run(["curl", "--fail", "--silent", "--show-error", "--max-time", "8", "--noproxy", "",
                "--proxy", f"http://alien:{self.token()}@{PROXY}:3128", "--cacert", str(self.output / "origin/ca.pem"),
                "https://allowed.test/"], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            assert result.returncode == 0, result.stderr
            equal(result.stdout, BODY)
        self.record("proxy URL credentials with real curl", proxy_url)
        def opaque(host, expected):
            with self.tunnel(host, 2222) as stream:
                stream.sendall(b"opaque-test\n")
                try:
                    data = stream.recv(1024)
                except ConnectionResetError:
                    data = b""
                if expected: assert data == b"opaque-test\n", data
                else: assert data != b"opaque-test\n", data
        self.record("explicit IP permits opaque protocol", lambda: opaque(ORIGIN, True))
        self.record("name alone cannot authorize opaque protocol", lambda: opaque("allowed.test", False))
        self.record("stored policy denies other name", lambda: self.tunnel("denied.test", expected=403))
        self.record("no declaration bound", lambda: equal(self.admin("/v1/policies", {"aud": AUD, "sub": "wider", "policy": "internet"})[0], 204))
        self.record("wider runtime policy accepted", lambda: equal(self.tls(host="denied.test", token=self.token(sub="wider")), (200, BODY)))
        self.record("empty allow policy installs", lambda: equal(self.admin("/v1/policies", {"aud": AUD, "sub": "closed", "policy": {"allow":[]}})[0], 204))
        self.record("empty allow identity passes readiness", lambda: equal(health(self.token(sub="closed"))[0], 200))
        self.record("stored policy denies allowed public name", lambda: self.tunnel(token=self.token(sub="closed"), expected=403))
        self.record("missing policy denies", lambda: self.tunnel(token=self.token(sub="missing"), expected=403))
        self.record("IP literal denied unless explicit", lambda: self.tunnel(ORIGIN, expected=403))
        self.record("SNI mismatch", lambda: self.denied_tls(sni="denied.test"))
        self.record("TLS on explicit port", lambda: equal(self.tls(port=9443), (200, BODY)))
        self.record("SNI mismatch on explicit port", lambda: self.denied_tls(port=9443, sni="denied.test"))
        def reject_hello(payload, delay=0):
            with self.tunnel() as stream:
                try:
                    stream.sendall(payload)
                    time.sleep(delay)
                    assert stream.recv(1024) == b"", "gateway forwarded a denied ClientHello"
                except ConnectionResetError:
                    pass
        # Generate only a ClientHello, with no server name. No TLS verification is skipped:
        # any server response is a test failure and the TLS session must never be established.
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        context.check_hostname = False
        incoming, outgoing = ssl.MemoryBIO(), ssl.MemoryBIO()
        hello = context.wrap_bio(incoming, outgoing)
        try:
            hello.do_handshake()
        except ssl.SSLWantReadError:
            pass
        self.record("missing SNI", lambda: reject_hello(outgoing.read()))
        self.record("oversized TLS record", lambda: reject_hello(b"\x16\x03\x01\xff\xff" + b"x" * 70000))
        self.record("slow ClientHello deadline", lambda: reject_hello(b"\x16", 5.2))
        self.record("explicit IP HTTP", lambda: equal(self.plain(target=ORIGIN, host=ORIGIN), (200, BODY)))
        self.record("forged HTTP Host", lambda: equal(self.plain(host="denied.test")[0], 403))
        self.record("CONNECT cannot bypass HTTP Host", lambda: equal(self.plain(host="denied.test", connect=True)[0], 403))
        private_control = 'import http.client; c=http.client.HTTPConnection("10.77.91.2",80,timeout=3);c.request("GET","/");r=c.getresponse();assert r.status==200 and r.read()==b"controlled-origin-response\\n";print("private listener reachable")'
        self.record("private listener control from gateway network", lambda: equal(docker("run", "--rm", "--name", PREFIX + "-private-control", "--network", "container:" + PREFIX + "-proxy1", "python:3.13-slim", "python", "-c", private_control), "private listener reachable"))
        for host in ["private.test", "mixed.test", "metadata.test", "configured-range.test", "overlay.test"]:
            self.record(host, lambda h=host: self.tunnel(h, expected=403))
        self.record("rebinding public control", lambda: equal(self.tls(host="rebind.test"), (200, BODY)))
        (self.output / "origin/rebind").write_text("10.77.91.2")
        self.record("rebinding private next connection", lambda: self.tunnel("rebind.test", expected=403))
        for name, overrides in [("expired", {"iat": 1, "exp": 2}), ("future", {"iat": int(time.time()) + 60}), ("wrong audience", {"aud": "other/box"}), ("excess lifetime", {"exp": int(time.time()) + 90000})]:
            self.record(name, lambda o=overrides: self.tunnel(token=self.token(**o), expected=403))
        forged = self.token().rsplit(".", 1)[0] + "." + encoded(bytes(64))
        self.record("forged signature", lambda: self.tunnel(token=forged, expected=403))
        self.record("admin token cannot proxy", lambda: self.tunnel(token=self.token(admin=True), expected=403))
        identity = {"aud": AUD, "sub": SUB}
        policy_body = {**identity, "policy": self.policy}
        self.record("proxy token cannot administer", lambda: equal(self.admin("/v1/policies", policy_body, self.token())[0], 401))
        self.record("signed admin policy cannot change", lambda: equal(self.admin("/v1/policies", {**policy_body, "policy":"internet"}, self.token(admin=True, method="PUT", policy=policy_digest(self.policy)))[0], 403))
        self.record("signed admin identity cannot change", lambda: equal(self.admin("/v1/policies", {**policy_body, "sub":"other"}, self.token(admin=True, method="PUT", policy=policy_digest(self.policy)))[0], 403))
        self.record("PUT grant cannot DELETE", lambda: equal(self.admin("/v1/policies", identity, self.token(admin=True, method="PUT", policy=policy_digest(self.policy)), method="DELETE")[0], 403))
        status, response = self.admin("/v1/ca", identity)
        assert status == 200, response
        ca = self.output / "guest-ca.pem"
        ca.write_text(json.loads(response)["certificate"])
        self.record("credential interception verified TLS", lambda: equal(self.tls(host="inject.test", ca=ca, path="/v1/action"), (200, BODY)))
        self.record("credential method scope", lambda: equal(self.tls(host="inject.test", ca=ca, path="/v1/action", method="POST"), (200, BODY)))
        self.record("credential path scope", lambda: equal(self.tls(host="inject.test", ca=ca, path="/outside"), (200, BODY)))
        self.record("credential Host forgery", lambda: equal(self.tls(host="inject.test", ca=ca, host_header="denied.test")[0], 403))
        self.record("credential path ambiguity", lambda: equal(self.tls(host="inject.test", ca=ca, path="/v1/%2e%2e/action")[0], 403))
        self.record("upstream TLS verification", lambda: self.denied_tls(host="inject.test", port=8443, ca=ca))
        requests = [json.loads(line) for line in (self.output / "origin/requests.jsonl").read_text().splitlines()]
        injected = [r for r in requests if "X-Test-Credential" in r["headers"] or "x-test-credential" in r["headers"]]
        assert len(injected) == 1, injected
        assert injected[0]["path"] == "/v1/action", injected
        assert {k.lower(): v for k, v in injected[0]["headers"].items()}["x-test-credential"] == "fake-test-secret"
        assert all("proxy-authorization" not in {k.lower() for k in r["headers"]} for r in requests)
        self.record("credential never returned to guest or forwarded outside scope", lambda: equal(len(injected), 1))

        def limit(payload, delay=0):
            with socket.create_connection((PROXY, 3128), timeout=8) as stream:
                try:
                    stream.sendall(payload)
                    time.sleep(delay)
                    assert stream.recv(1024) == b""
                except ConnectionResetError:
                    pass
        self.record("oversized header", lambda: limit(b"CONNECT " + b"a" * 220000))
        self.record("slow header deadline", lambda: limit(b"CONNECT ", 5.2))
        unchanged_token = self.token()
        self.record("replica policy control", lambda: self.tunnel(proxy="93.184.216.4").close())
        equal(self.admin("/v1/policies", {**identity, "policy":{"allow":["blocked.example.test"]}})[0], 204)
        self.record("same identity token observes replacement", lambda: self.tunnel(token=unchanged_token, expected=403))
        self.record("second replica observes replacement", lambda: self.tunnel(token=unchanged_token, proxy="93.184.216.4", expected=403))
        equal(self.admin("/v1/policies", policy_body)[0], 204)
        self.record("same token observes widening", lambda: equal(self.tls(token=unchanged_token), (200, BODY)))
        equal(self.admin("/v1/policies", identity, method="DELETE")[0], 204)
        self.record("deleted policy denies previously accepted token", lambda: self.tunnel(token=unchanged_token, expected=403))
        self.record("second replica denies deleted policy", lambda: self.tunnel(token=unchanged_token, proxy="93.184.216.4", expected=403))
        equal(self.admin("/v1/policies", policy_body)[0], 204)
        large = {"allow": [f"host{i}.example.test" for i in range(4096)]}
        self.record("4096-target policy delivered over admin HTTP", lambda: equal(self.admin("/v1/policies", {"aud": AUD, "sub": "large", "policy": large})[0], 204))
        self.record("small token resolves 4096-target policy", lambda: equal(self.tls(host="host4095.example.test", token=self.token(sub="large")), (200, BODY)))
        status, error = self.admin("/v1/policies", {"aud": AUD, "sub": "too-large", "policy": {"allow": large["allow"] + ["extra.example.test"]}})
        self.record("4097 targets refused with typed error", lambda: equal((status, json.loads(error)["code"]), (400, "SANDBOX_EGRESS_POLICY_TOO_LARGE")))
        self.benchmark()

    def benchmark(self):
        result = {}
        for direct in [True, False]:
            timings = []
            for _ in range(30):
                started = time.perf_counter()
                assert self.tls(direct=direct, token=self.token()) == (200, BODY)
                timings.append((time.perf_counter() - started) * 1000)
            started = time.perf_counter()
            for _ in range(5):
                status, body = self.tls(direct=direct, token=self.token(), path="/large")
                assert status == 200 and len(body) == 8 * 1024 * 1024
            elapsed = time.perf_counter() - started
            result["direct" if direct else "gateway"] = {"p50Ms": statistics.median(timings), "p95Ms": sorted(timings)[28], "throughputMiBps": 40 / elapsed}
        (self.output / "benchmark.json").write_text(json.dumps(result, indent=2))
        print(json.dumps({"benchmark": result}), flush=True)

    def agent(self, image, containers, networks):
        def equal(actual, expected):
            assert actual == expected, (actual, expected)
        # The image provides ordinary app tools only; the agent owns firewall installation.
        self.record("agent image needs no nft or iptables", lambda: equal(docker(
            "run", "--rm", "--entrypoint", "python", image, "-c",
            "import shutil; assert all(shutil.which(x) is None for x in ['nft','iptables','iptables-nft']); print('no firewall tools')"
        ), "no firewall tools"))
        failed = PREFIX + "-no-net-admin"
        failed_root = self.output / "failed-agent"
        failed_root.mkdir(mode=0o777)
        failed_root.chmod(0o777)
        docker("run", "-d", "--name", failed, "--network", "none",
               "-v", f"{failed_root}:/data", image)
        containers.append(failed)
        def cannot_install():
            deadline = time.monotonic() + 10
            while docker("inspect", "--format", "{{.State.Running}}", failed) == "true":
                assert time.monotonic() < deadline, "agent did not fail closed without NET_ADMIN"
                time.sleep(.05)
            assert int(docker("inspect", "--format", "{{.State.ExitCode}}", failed)) != 0
            assert "install transparent forwarding" in docker("logs", failed)
            assert not (failed_root / "image-env").exists(), "image command started without redirect"
        self.record("refused netlink installation exits before serving or image code", cannot_install)
        network = PREFIX + "-guest"
        docker("network", "create", "--internal", "--subnet", "172.29.91.0/24", network)
        networks.append(network)
        docker("network", "connect", "--ip", "172.29.91.2", "--alias", "gateway.test", network, PREFIX + "-proxy1")
        name = PREFIX + "-agent"
        docker("run", "-d", "--name", name, "--network", network, "--ip", "172.29.91.3", *AGENT_CAP_ARGS,
               "-v", f"{self.output / 'origin/ca.pem'}:/fixture-ca.pem:ro", image)
        containers.append(name)

        def call(method, path, body=None):
            conn = http.client.HTTPConnection("172.29.91.3", 8080, timeout=12)
            conn.request(method, path, json.dumps(body) if body is not None else None, {"Content-Type": "application/json"})
            response = conn.getresponse()
            result = response.status, response.read()
            conn.close()
            return result

        deadline = time.monotonic() + 10
        while True:
            try:
                status, health = call("GET", "/v1/health")
                assert status == 200 and json.loads(health)["status"] == "awaiting-egress-policy", (status, health)
                break
            except OSError:
                if time.monotonic() > deadline:
                    raise
                time.sleep(0.1)
        self.record("agent pending health", lambda: equal(json.loads(call("GET", "/v1/health")[1])["status"], "awaiting-egress-policy"))
        hook = {"microvmId": SUB, "runHookPayload": json.dumps({"gateway": "172.29.91.2:3128", "audience": AUD})}
        self.record("AWS run wrapper bootstrap", lambda: equal(call("POST", "/aws/lambda-microvms/runtime/v1/run", hook)[0], 200))
        # Resolve through the marked control socket after the guest DNS redirect is active.
        # The subsequent token self-check must reach the real gateway, not a synthetic IP.
        hook["runHookPayload"] = json.dumps({"gateway": "gateway.test:3128", "audience": AUD})
        self.record("AWS run hook resolves gateway hostname through control DNS", lambda: equal(call("POST", "/aws/lambda-microvms/runtime/v1/run", hook)[0], 200))
        self.record("agent refuses other sandbox token", lambda: equal(call("PUT", "/v1/egress", {"token": self.token(sub="other")})[0], 500))
        forged = self.token().rsplit(".", 1)[0] + "." + encoded(bytes(64))
        self.record("agent refuses forged identity via authenticated health", lambda: equal(call("PUT", "/v1/egress", {"token": forged})[0], 503))
        self.record("agent token install and path self-check", lambda: equal(call("PUT", "/v1/egress", {"token": self.token()})[0], 204))
        self.record("agent ready health", lambda: equal(json.loads(call("GET", "/v1/health")[1])["status"], "ready"))
        check_capabilities(self, name, call, docker, AGENT_UID)
        def child_socket_boundary():
            # Docker exec bypasses the agent's child setup: establish that IPv6 exists
            # in this namespace, then exercise the real command launch boundary.
            docker("exec", name, "python", "-c", "import socket; socket.socket(socket.AF_INET6).close()")
            program = '''import socket
for kind in [socket.SOCK_STREAM, socket.SOCK_DGRAM]:
 try: socket.socket(socket.AF_INET6, kind)
 except PermissionError: pass
 else: raise AssertionError("IPv6 escaped the command boundary")
socket.socket(socket.AF_INET, socket.SOCK_STREAM).close()
print("IPv6 blocked; IPv4 available")
'''
            status, body = call("POST", "/v1/exec", {"command": ["python", "-c", program], "timeoutMs": 5000})
            assert status == 200, (status, body)
            frames = [json.loads(line) for line in body.splitlines()]
            assert frames[-1]["t"] == "exit" and frames[-1]["code"] == 0, frames
            output = b"".join(base64.b64decode(frame["data"]) for frame in frames if frame["t"] == "stdout")
            assert output.strip() == b"IPv6 blocked; IPv4 available", frames
        self.record("actual exec blocks IPv6 despite namespace socket support", child_socket_boundary)
        def reinstall():
            # A second bootstrap replaces the existing tables before failing to bind the
            # occupied control port. Subsequent packet probes exercise the replacement rules.
            result = subprocess.run(
                ["docker", "exec", "--privileged", name, "alien-sandbox-agent"],
                env={**os.environ, "DOCKER_HOST": "unix:///var/run/docker.sock"},
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=10,
            )
            assert result.returncode != 0 and b"bind the agent listener" in result.stdout, result.stdout
            assert b"install transparent forwarding" not in result.stdout, result.stdout
        self.record("bootstrap atomically replaces its existing tables", reinstall)
        image_env = docker("exec", "-u", f"{AGENT_UID}:{AGENT_UID}", name, "cat", "/data/image-env")
        assert "HTTP_PROXY=http://127.0.0.1:15002" in image_env
        assert "ALIEN_SANDBOX_EGRESS_TOKEN" not in image_env and "fake-test-secret" not in image_env
        self.record("gateway command environment preserves localhost HTTP", lambda: check_loopback_http(name))
        self.record("remote client reaches command UID preview", lambda: check_remote_preview(name, f"{AGENT_UID}:{AGENT_UID}", 9090))
        self.record("remote client reaches root preview", lambda: check_remote_preview(name, "0", 9091))
        new_connection = '''import socket
s=socket.socket();s.settimeout(3);s.bind(("0.0.0.0",9092))
try:
 s.connect(("172.29.91.2",3128));s.sendall(b"GET /health HTTP/1.1\\r\\nHost: gateway\\r\\n\\r\\n")
 data=s.recv(1024)
 assert not data, data
except (ConnectionError,TimeoutError): pass
print("new outbound refused")
'''
        self.record("new outbound TCP cannot use incoming reply exception", lambda: equal(docker("exec", "-u", f"{AGENT_UID}:{AGENT_UID}", name, "python", "-c", new_connection), "new outbound refused"))
        script = '''import socket,ssl,http.client
ip=socket.gethostbyname("allowed.test")
assert ip.startswith("198.18."),ip
s=socket.create_connection(("allowed.test",443),timeout=8)
c=ssl.create_default_context(cafile="/fixture-ca.pem")
with c.wrap_socket(s,server_hostname="allowed.test") as t:
 t.sendall(b"GET / HTTP/1.1\\r\\nHost: allowed.test\\r\\nConnection: close\\r\\n\\r\\n")
 r=http.client.HTTPResponse(t);r.begin();assert r.status==200 and r.read()==b"controlled-origin-response\\n"
print("command transparent TLS passed")
'''
        check_system_egress(self, name, call, docker, AGENT_UID, PREFIX)
        self.record("command UID TCP uses synthetic DNS and gateway", lambda: equal(docker("exec", "-u", f"{AGENT_UID}:{AGENT_UID}", name, "python", "-c", script), "command transparent TLS passed"))
        equal(call("PUT", "/v1/egress", {"token": self.token(), "caCert": (self.output / "guest-ca.pem").read_text()})[0], 204)
        injected_script = script.replace('"allowed.test"', '"inject.test"').replace('/fixture-ca.pem', '/opt/alien/egress-ca-bundle.pem').replace('GET / HTTP', 'GET /v1/action HTTP').replace('Host: allowed.test', 'Host: inject.test')
        self.record("command UID uses installed CA for intercepted TLS", lambda: equal(docker("exec", "-u", f"{AGENT_UID}:{AGENT_UID}", name, "python", "-c", injected_script), "command transparent TLS passed"))
        memory_check = '''try:
 open("/proc/1/mem","rb")
except PermissionError: print("agent memory inaccessible")
else: raise AssertionError("command can read agent memory")
'''
        self.record("command UID cannot read agent memory", lambda: equal(docker("exec", "-u", f"{AGENT_UID}:{AGENT_UID}", name, "python", "-c", memory_check), "agent memory inaccessible"))
        docker("exec", "-u", f"{AGENT_UID}:{AGENT_UID}", name, "python", "-c", 'import socket; assert socket.gethostbyname("exfil-test.test").startswith("198.18.")')
        queries = [json.loads(line)["name"] for line in (self.output / "origin/dns.jsonl").read_text().splitlines()]
        assert "allowed.test" in queries and "exfil-test.test" not in queries
        self.record("guest DNS stays synthetic with working gateway DNS control", lambda: equal("exfil-test.test" in queries, False))
        self.record("live policy replacement without agent PUT", lambda: equal(self.admin("/v1/policies", {"aud": AUD, "sub": SUB, "policy": {"allow":["blocked.example.test"]}})[0], 204))
        denied = '''import socket,ssl
s=socket.create_connection(("allowed.test",443),timeout=8)
try:
 ssl.create_default_context(cafile="/fixture-ca.pem").wrap_socket(s,server_hostname="allowed.test")
except (ssl.SSLError,ConnectionError): print("denied")
else: raise AssertionError("unexpected egress")
'''
        self.record("command UID honors stored policy replacement", lambda: equal(docker("exec", "-u", f"{AGENT_UID}:{AGENT_UID}", name, "python", "-c", denied), "denied"))

        for mode, expected in [("none", 200), ("bridge", 503)]:
            closed = PREFIX + f"-floor-{mode}"
            docker("run", "-d", "--name", closed, "--network", mode, *AGENT_CAP_ARGS, "-e", 'ALIEN_SANDBOX_EGRESS="none"', image)
            containers.append(closed)
            health_script = '''import http.client,json,time
deadline=time.monotonic()+8
while True:
 try:
  c=http.client.HTTPConnection("127.0.0.1",8080,timeout=1);c.request("GET","/v1/health");r=c.getresponse();print(r.status);break
 except OSError:
  if time.monotonic()>deadline:raise
  time.sleep(.05)
'''
            self.record(f"declared none floor {mode}", lambda n=closed, e=expected: equal(docker("exec", n, "python", "-c", health_script), str(e)))
            if mode == "none":
                self.record("declared none command environment preserves localhost HTTP", lambda: check_loopback_http(closed))
            if mode == "bridge":
                control = 'import socket; s=socket.socket();s.setsockopt(socket.SOL_SOCKET,36,0xa11e);s.settimeout(3);s.connect(("1.1.1.1",443));print("reachable")'
                self.record("open floor direct canary positive control", lambda: equal(docker("exec", closed, "python", "-c", control), "reachable"))


    def proxy_only(self, image, containers):
        name = PREFIX + "-proxy-only"
        sub = "proxy-only"
        assert self.admin("/v1/policies", {"aud": AUD, "sub": sub, "policy": self.policy})[0] == 204
        docker("run", "-d", "--name", name, "--network", PREFIX + "-guest", "--ip", "172.29.91.4",
               "--user", f"{AGENT_UID}:{AGENT_UID}", "--cap-drop", "ALL", "--read-only",
               "--tmpfs", f"/data:rw,uid={AGENT_UID},gid={AGENT_UID},mode=0700",
               "-e", "ALIEN_SANDBOX_ISOLATION=platform", "-e", "ALIEN_SANDBOX_SESSION_ID=" + sub,
               "-e", "ALIEN_SANDBOX_EGRESS_AUDIENCE=" + AUD, "-e", "ALIEN_SANDBOX_EGRESS_GATEWAY=172.29.91.2:3128",
               "-v", f"{self.output / 'origin/ca.pem'}:/fixture-ca.pem:ro", image)
        containers.append(name)
        def call(method, path, body=None):
            conn = http.client.HTTPConnection("172.29.91.4", 8080, timeout=10)
            conn.request(method, path, json.dumps(body) if body is not None else None, {"Content-Type":"application/json"})
            response = conn.getresponse()
            result = response.status, response.read()
            conn.close()
            return result
        deadline = time.monotonic() + 10
        while True:
            try:
                status, body = call("GET", "/v1/health")
                assert status == 200 and json.loads(body)["status"] == "awaiting-egress-policy", (status, body)
                break
            except OSError:
                if time.monotonic() > deadline: raise
                time.sleep(.1)
        def install():
            status, body = call("PUT", "/v1/egress", {"token":self.token(sub=sub)})
            assert status == 204, (status, body)
        self.record("unprivileged agent installs identity and verifies floor", install)
        script = '''import os,urllib.request,socket
from pathlib import Path
env=dict(line.split("=",1) for line in Path("/data/image-env").read_text().splitlines())
os.environ.clear();os.environ.update(env)
import ssl
context=ssl.create_default_context(cafile="/fixture-ca.pem")
assert urllib.request.urlopen("https://allowed.test/",context=context,timeout=8).read()==b"controlled-origin-response\\n"
assert "ALIEN_SANDBOX_EGRESS_TOKEN" not in env
s=socket.socket();s.settimeout(2)
try:s.connect(("93.184.216.2",443))
except OSError:pass
else:raise AssertionError("direct socket escaped proxy-only floor")
print("proxy-only passed")
'''
        def probe():
            assert docker("exec", name, "python", "-c", script) == "proxy-only passed"
        self.record("unprivileged HTTPS_PROXY works and direct socket fails", probe)
        self.record("unprivileged localhost HTTP remains local", lambda: check_loopback_http(name))


def main():
    global AGENT_UID
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--image", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--agent-image")
    parser.add_argument("--agent-uid", type=int, default=60000, help="command UID/GID configured in the agent fixture image")
    parser.add_argument("--gateway-binary", type=Path)
    parser.add_argument("--vault-config", action="store_true")
    args = parser.parse_args()
    if args.agent_uid <= 0:
        parser.error("--agent-uid must be a non-root UID")
    AGENT_UID = args.agent_uid
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    for name in ["origin", "vault", "kv"]:
        (output / name).mkdir(mode=0o777)
        (output / name).chmod(0o777)
    ca = certificates(output / "origin")
    (output / "origin/rebind").write_text(ORIGIN)
    (output / "vault/secrets.json").write_text(json.dumps({"test": "fake-test-secret"}))
    suite = Suite(output)
    public = base64.b64encode(suite.key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)).decode()
    allowed = ["allowed.test", f"{ORIGIN}:80", f"{ORIGIN}:2222", "inject.test", "inject.test:8443", "rebind.test", "private.test", "mixed.test", "metadata.test", "configured-range.test", "overlay.test", "*.example.test"]
    suite.policy = {"allow": allowed}
    rules = [dict(aud=AUD, host="inject.test", port=port, method="GET", path="/v1/", header="X-Test-Credential", vaultBinding="test-vault", secret="test") for port in [443, 8443]]
    config = dict(listen="0.0.0.0:3128", audiences=[AUD], defaultPolicies={AUD: "internet"}, generationStore={"binding":"egress-state"}, keys=[dict(kid="test", publicKey=public)], deniedRanges=["93.184.217.0/24", "10.244.0.0/16"], credentials=rules, upstreamCaCertificates=[ca])
    if args.vault_config:
        (output / "vault/secrets.json").write_text(json.dumps({"test":"fake-test-secret", "sandbox-egress-gateway-config":json.dumps(config)}))
    containers, networks = [], []
    try:
        docker("network", "create", "--internal", "--subnet", "93.184.216.0/24", PREFIX)
        networks.append(PREFIX)
        private_network = PREFIX + "-private"
        docker("network", "create", "--internal", "--subnet", "10.77.91.0/24", private_network)
        networks.append(private_network)
        name = PREFIX + "-origin"
        docker("run", "-d", "--name", name, "--network", PREFIX, "--ip", ORIGIN,
               "-v", f"{output / 'origin'}:/fixture", "-v", f"{Path(__file__).with_name('origin.py')}:/origin.py:ro", "python:3.13-slim", "python", "/origin.py")
        containers.append(name)
        docker("network", "connect", "--ip", "10.77.91.2", private_network, name)
        for number, ip in [(1, PROXY), (2, "93.184.216.4")]:
            name = PREFIX + f"-proxy{number}"
            binary = ["-v", f"{args.gateway_binary.resolve()}:/usr/local/bin/alien-egress-gateway:ro"] if args.gateway_binary else []
            config_env = ["-e", "ALIEN_SECRETS_BINDING=" + json.dumps({"service":"local-vault","dataDir":"/vault","vaultName":"secrets"})] if args.vault_config else ["-e", "ALIEN_EGRESS_GATEWAY_CONFIG=" + json.dumps(config)]
            docker("run", "-d", "--read-only", "--cap-drop", "ALL", *binary, "--name", name, "--network", PREFIX, "--ip", ip, "--dns", ORIGIN,
                   "-v", f"{output / 'kv'}:/kv", "-v", f"{output / 'vault'}:/vault:ro",
                   "-e", "ALIEN_DEPLOYMENT_TYPE=local", "-e", "ALIEN_EGRESS_STATE_BINDING=" + json.dumps({"service": "local-kv", "dataDir": "/kv"}),
                   "-e", "ALIEN_TEST_VAULT_BINDING=" + json.dumps({"service": "local-vault", "dataDir": "/vault", "vaultName": "test"}),
                   *config_env, args.image)
            containers.append(name)
            docker("network", "connect", "--ip", f"10.77.91.{number + 2}", private_network, name)
        deadline = time.monotonic() + 30
        while True:
            try:
                conn = http.client.HTTPConnection(PROXY, 3129, timeout=1)
                conn.request("GET", "/health")
                assert conn.getresponse().read() == b"ok\n"
                break
            except (OSError, http.client.HTTPException):
                if time.monotonic() > deadline:
                    raise
                time.sleep(0.1)
        suite.run()
        if args.agent_image:
            from legacy_egress import check_legacy_agents
            check_legacy_agents(suite, args.agent_image, containers, AGENT_UID)
            suite.agent(args.agent_image, containers, networks)
            suite.proxy_only(args.agent_image, containers)
            from run_hook_egress import check_run_hook_agents
            check_run_hook_agents(suite, args.agent_image, containers, AGENT_UID)
        if args.vault_config:
            assert suite.admin("/v1/policies", {"aud":AUD,"sub":"reload","policy":"internet"})[0] == 204
            suite.tunnel(token=suite.token(sub="reload")).close()
            config["deniedRanges"].append(ORIGIN + "/32")
            secret_file = output / "vault/secrets.json"
            replacement = secret_file.with_suffix(".next")
            replacement.write_text(json.dumps({"test":"fake-test-secret", "sandbox-egress-gateway-config":json.dumps(config)}))
            replacement.replace(secret_file)
            def reload_proof():
                deadline = time.monotonic() + 75
                pending = {PROXY, "93.184.216.4"}
                while pending:
                    for proxy in list(pending):
                        conn = http.client.HTTPConnection(proxy, 3128, timeout=3)
                        conn.request("CONNECT", "allowed.test:443", headers={"Host":"allowed.test:443", "Proxy-Authorization":"Bearer " + suite.token(sub="reload")})
                        response = conn.getresponse()
                        if response.status == 403: pending.remove(proxy)
                        else: assert response.status == 200, response.status
                        conn.close()
                    if pending:
                        assert time.monotonic() < deadline, pending
                        time.sleep(1)
            suite.record("vault config polls replace CIDRs on both running replicas", reload_proof)
    finally:
        secret_in_logs = False
        for name in reversed(containers):
            logs = docker("logs", name)
            (output / f"{name}.log").write_text(logs)
            secret_in_logs |= "fake-test-secret" in logs
            docker("rm", "-f", name)
        for name in networks:
            docker("network", "rm", name)
        remaining = docker("ps", "-a", "--filter", f"name={PREFIX}", "--format", "{{.Names}}")
        assert remaining == "", remaining
        assert not secret_in_logs, "credential leaked to container logs"
        (output / "cleanup.txt").write_text("Owned containers and network deleted; scoped container list empty.\n")


if __name__ == "__main__":
    main()
