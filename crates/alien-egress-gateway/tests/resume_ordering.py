"""Real agent lifecycle ordering and concurrent session admission."""
import concurrent.futures
import json
import time


RESUME = "/aws/lambda-microvms/runtime/v1/resume"


def check_resume_ordering(suite, name, call, docker, networks, prefix):
    def exec_ok():
        status, body = call("POST", "/v1/exec", {"command": ["true"], "timeoutMs": 1000})
        assert status == 200, (status, body)
        frames = [json.loads(line) for line in body.splitlines()]
        assert frames[-1]["t"] == "exit" and frames[-1]["code"] == 0, frames

    def delivery_before_hook():
        assert call("PUT", "/v1/egress", {"token": suite.token()})[0] == 204
        assert call("POST", RESUME, {})[0] == 200
        exec_ok()
    suite.record("delivery before late Resume hook leaves commands ready", delivery_before_hook)

    def during_hook():
        with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
            resume = pool.submit(call, "POST", RESUME, {})
            deadline = time.monotonic() + 2
            while True:
                status, body = call("GET", "/v1/health")
                assert status == 200, (status, body)
                if json.loads(body)["status"] == "verifying-egress-path":
                    break
                assert not resume.done() and time.monotonic() < deadline, "check never became observable"
                time.sleep(.01)
            command = pool.submit(exec_ok)
            delivery = pool.submit(call, "PUT", "/v1/egress", {"token": suite.token()})
            time.sleep(.1)
            assert not command.done(), "exec did not wait for the path check"
            assert not delivery.done(), "delivery did not wait for the path check"
            assert resume.result()[0] == 200
            command.result()
            assert delivery.result()[0] == 204
    suite.record("exec and delivery during Resume wait for verified readiness", during_hook)

    network = prefix + "-resume-direct"
    docker("network", "create", "--internal", "--subnet", "1.1.1.0/24", "--gateway", "1.1.1.254", network)
    networks.append(network)
    docker("network", "connect", "--ip", "1.1.1.1", network, prefix + "-origin")
    docker("network", "connect", "--ip", "1.1.1.10", network, name)
    try:
        def direct_refused():
            # Positive control: the strict supervisor TCP canary really is reachable.
            docker("exec", name, "python", "-c", 'import socket;socket.create_connection(("1.1.1.1",443),timeout=2).close()')
            assert call("POST", RESUME, {})[0] == 503
            assert call("GET", "/v1/health")[0] == 503
            assert call("POST", "/v1/exec", {"command": ["true"], "timeoutMs": 1000})[0] == 503
        suite.record("Resume strictly refuses reachable direct-internet canary", direct_refused)
    finally:
        docker("network", "disconnect", network, name)
        docker("network", "disconnect", network, prefix + "-origin")
    assert call("POST", RESUME, {})[0] == 200
    exec_ok()

    def expired_identity():
        expiry = int(time.time()) + 8
        assert call("PUT", "/v1/egress", {"token": suite.token(exp=expiry)})[0] == 204
        while time.time() < expiry:
            time.sleep(.05)
        assert call("POST", RESUME, {})[0] == 200
        status, body = call("GET", "/v1/health")
        assert status == 200 and json.loads(body)["status"] == "awaiting-egress-policy", (status, body)
        assert call("POST", "/v1/exec", {"command": ["true"], "timeoutMs": 1000})[0] == 503
        assert call("PUT", "/v1/egress", {"token": suite.token()})[0] == 204
        exec_ok()
    suite.record("Resume with an expired identity stays closed until renewal", expired_identity)


def check_run_ordering(suite, image, containers, networks, prefix, audience):
    import http.client
    from agent_capabilities import AGENT_CAP_ARGS
    from docker_conformance import docker

    network = prefix + "-early-run"
    docker("network", "create", "--internal", "--subnet", "172.29.93.0/24", network)
    networks.append(network)
    docker("network", "connect", "--ip", "172.29.93.2", network, prefix + "-proxy1")
    name = prefix + "-early-agent"
    docker("run", "-d", "--name", name, "--network", network, "--ip", "172.29.93.3", *AGENT_CAP_ARGS,
           "-e", "ALIEN_SANDBOX_EGRESS_RUN_HOOK=true", image)
    containers.append(name)

    def call(method, path, body=None):
        connection = http.client.HTTPConnection("172.29.93.3", 8080, timeout=25)
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
            assert time.monotonic() < deadline, "agent never started"
            time.sleep(.05)
    assert suite.admin("/v1/policies", {"aud": audience, "sub": name, "policy": "internet"})[0] == 204
    delivery = suite.delivery({"token": suite.token(sub=name)})

    def early_delivery():
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            pending = pool.submit(call, "PUT", "/v1/egress", delivery)
            time.sleep(.2)
            assert not pending.done(), ("early delivery refused instead of waiting", pending.result())
            hook = {"microvmId": name, "runHookPayload": json.dumps({"gateway": "172.29.93.2:3128", "audience": audience})}
            status, body = call("POST", "/aws/lambda-microvms/runtime/v1/run", hook)
            assert status == 200, (status, body)
            status, body = pending.result()
            assert status == 204, (status, body)
        status, body = call("POST", "/v1/exec", {"command": ["true"], "timeoutMs": 1000})
        assert status == 200, (status, body)
        assert json.loads(body.splitlines()[-1])["code"] == 0, body
    suite.record("identity delivery before Run waits then permits first exec", early_delivery)
