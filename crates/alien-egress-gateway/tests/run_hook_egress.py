"""Exercise delayed firewall installation with real netlink and HTTP requests."""

import http.client
import json
import time

from agent_capabilities import AGENT_CAP_ARGS
from docker_conformance import AUD, PREFIX, docker


def check_run_hook_agents(suite, image, containers, uid):
    # Keep NET_ADMIN but reject creation of netlink sockets: startup still serves the
    # hook, whose actual firewall transaction must fail without starting guest code.
    seccomp = suite.output / "refuse-netlink.json"
    seccomp.write_text(json.dumps({
        "defaultAction": "SCMP_ACT_ALLOW",
        "syscalls": [{"names": ["socket"], "action": "SCMP_ACT_ERRNO", "errnoRet": 1,
                      "args": [{"index": 0, "value": 16, "op": "SCMP_CMP_EQ"}]}],
    }))
    for mode in ["internet", "netlink-failure", "none", "open-floor"]:
        name = PREFIX + "-hook-" + mode
        declared = "none" if mode in ["none", "open-floor"] else "internet"
        network = "bridge" if mode == "open-floor" else PREFIX + "-guest"
        options = ["--security-opt", "seccomp=" + str(seccomp)] if mode == "netlink-failure" else []
        docker("run", "-d", "--name", name, "--network", network, *AGENT_CAP_ARGS,
               "-e", "ALIEN_SANDBOX_EGRESS_RUN_HOOK=true", "-e", "ALIEN_SANDBOX_EGRESS=" + declared,
               "-v", f"{suite.output / 'origin/ca.pem'}:/fixture-ca.pem:ro", *options, image)
        containers.append(name)
        address = json.loads(docker("inspect", name))[0]["NetworkSettings"]["Networks"][network]["IPAddress"]

        def call(method, path, body=None):
            if method == "PUT" and path == "/v1/egress": body = suite.delivery(body)
            connection = http.client.HTTPConnection(address, 8080, timeout=12)
            try:
                connection.request(method, path, json.dumps(body) if body is not None else None,
                                   {"Content-Type": "application/json"})
                response = connection.getresponse()
                return response.status, response.read()
            finally:
                connection.close()

        deadline = time.monotonic() + 10
        while True:
            try:
                status, body = call("GET", "/v1/health")
                assert status == 200 and json.loads(body)["status"] == "awaiting-run-hook", (status, body)
                break
            except OSError:
                assert time.monotonic() < deadline, "deferred agent did not start"
                time.sleep(.05)

        def guest_stopped():
            assert docker("exec", name, "python", "-c",
                          "from pathlib import Path; assert not Path('/data/image-env').exists()") == ""
            status, _ = call("POST", "/v1/exec", {"command": ["touch", "/data/exec-ran"], "timeoutMs": 1000})
            assert status == 503, status
            assert docker("exec", name, "python", "-c",
                          "from pathlib import Path; assert not Path('/data/exec-ran').exists()") == ""

        suite.record(f"Run-hook {mode}: image and exec blocked before firewall", guest_stopped)
        token = suite.token(sub=name)
        assert call("PUT", "/v1/egress", {"token": token})[0] == 503
        if network != "bridge":
            # The trusted platform can use DNS/TCP until Run. An already-installed
            # guest redirect would return a synthetic DNS address here.
            suite.record(f"Run-hook {mode}: bootstrap DNS remains untouched", lambda: docker(
                "exec", name, "python", "-c",
                "import socket; assert socket.gethostbyname('gateway.test') == '172.29.91.2'"
            ))
        hook = {"microvmId": name, "runHookPayload": json.dumps({"gateway": "gateway.test:3128", "audience": AUD})}
        expected = 500 if mode == "netlink-failure" else 503 if mode == "open-floor" else 200

        def run():
            status, body = call("POST", "/aws/lambda-microvms/runtime/v1/run", hook)
            assert status == expected, (status, body)

        suite.record(f"Run-hook {mode}: install and floor result", run)
        if mode in ["netlink-failure", "open-floor"]:
            suite.record(f"Run-hook {mode}: failed activation keeps guest stopped", guest_stopped)
            continue
        if mode == "internet":
            suite.record("Run-hook internet: installed firewall still awaits token", guest_stopped)
            assert suite.admin("/v1/policies", {"aud": AUD, "sub": name, "policy": {"allow": ["allowed.test"]}})[0] == 204
            status, body = call("PUT", "/v1/egress", {"token": token})
            assert status == 204, (status, body)
            probe = '''import socket,ssl,http.client
assert socket.gethostbyname("allowed.test").startswith("198.18.")
with ssl.create_default_context(cafile="/fixture-ca.pem").wrap_socket(socket.create_connection(("allowed.test",443),timeout=8),server_hostname="allowed.test") as s:
 s.sendall(b"GET / HTTP/1.1\\r\\nHost: allowed.test\\r\\nConnection: close\\r\\n\\r\\n")
 r=http.client.HTTPResponse(s);r.begin();assert r.status==200 and r.read()==b"controlled-origin-response\\n"
'''
            suite.record("Run-hook internet: command TCP uses installed redirect", lambda: docker("exec", "-u", f"{uid}:{uid}", name, "python", "-c", probe))
        status, body = call("GET", "/v1/health")
        assert status == 200 and json.loads(body)["status"] == "ready", (status, body)
        suite.record(f"Run-hook {mode}: image starts only after activation", lambda: docker(
            "exec", name, "python", "-c", "from pathlib import Path; assert Path('/data/image-env').exists()"
        ))
