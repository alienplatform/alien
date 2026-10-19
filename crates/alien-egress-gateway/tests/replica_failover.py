"""Kill one gateway replica behind a load balancer while a sandbox sends steady traffic.

The balancer models a cloud network load balancer: every new connection goes to a random
registered replica, and a replica is removed only after UNHEALTHY consecutive failed health
checks INTERVAL seconds apart. Until then, connections sent to the killed replica fail when
the network gives up on it. Any one sandbox policy check can land there, but egress must stay
open while the other replica answers, and unlisted destinations must stay denied throughout.
"""
import datetime
import json
import re
import subprocess
import threading
import time
import http.client

from agent_capabilities import AGENT_CAP_ARGS

INTERVAL = 10
UNHEALTHY = 3
KILL_AFTER = 15
DURATION = 75
BALANCER_IP = "172.29.91.2"
AGENT_IP = "172.29.91.3"
REPLICAS = ["93.184.216.3:3128", "93.184.216.4:3128"]
ALLOWED, UNLISTED = "allowed.test", "denied.test"

BALANCER = r'''
import json, random, socket, sys, threading, time
targets = sys.argv[1].split(",")
interval, unhealthy = float(sys.argv[2]), int(sys.argv[3])
registered, failures, lock = set(targets), {target: 0 for target in targets}, threading.Lock()
output = threading.Lock()

def log(**event):
    with output:
        print(json.dumps({"t": time.time(), **event}), flush=True)

def health():
    while True:
        time.sleep(interval)
        for target in targets:
            try:
                with socket.create_connection((target.split(":")[0], 3129), timeout=interval / 2) as probe:
                    probe.sendall(b"GET /health HTTP/1.1\r\nHost: balancer\r\nConnection: close\r\n\r\n")
                    healthy = probe.recv(64).startswith(b"HTTP/1.1 200")
            except OSError:
                healthy = False
            with lock:
                failures[target] = 0 if healthy else failures[target] + 1
                if failures[target] >= unhealthy and target in registered:
                    registered.discard(target)
                    log(event="deregistered", target=target)

def pipe(source, destination):
    try:
        while data := source.recv(65536):
            destination.sendall(data)
        destination.shutdown(socket.SHUT_WR)
    except OSError:
        pass

def handle(client):
    with lock:
        target = random.choice(sorted(registered))
    started = time.time()
    try:
        upstream = socket.create_connection((target.split(":")[0], int(target.split(":")[1])), timeout=10)
    except OSError as error:
        log(event="target-failed", target=target, error=type(error).__name__, seconds=round(time.time() - started, 3))
        client.close()
        return
    upstream.settimeout(None)
    forward = threading.Thread(target=pipe, args=(client, upstream), daemon=True)
    forward.start()
    pipe(upstream, client)
    forward.join()
    client.close()
    upstream.close()

threading.Thread(target=health, daemon=True).start()
listener = socket.create_server(("0.0.0.0", 3128), backlog=512)
log(event="listening", targets=targets)
while True:
    client, _ = listener.accept()
    threading.Thread(target=handle, args=(client,), daemon=True).start()
'''

# Runs as the command UID through the agent's explicit proxy, like an ordinary proxy-aware app.
LOOP = r'''
import http.client, json, socket, ssl, sys, time
context = ssl.create_default_context(cafile="/fixture-ca.pem")
end, sequence = time.time() + float(sys.argv[1]), 0
while time.time() < end:
    for target in ["allowed.test", "denied.test"]:
        started = time.time()
        try:
            stream = socket.create_connection(("127.0.0.1", 15002), timeout=5)
            stream.settimeout(5)
            stream.sendall(f"CONNECT {target}:443 HTTP/1.1\r\nHost: {target}:443\r\n\r\n".encode())
            head = b""
            while b"\r\n\r\n" not in head:
                part = stream.recv(1)
                if not part:
                    raise ConnectionResetError("proxy closed before a status")
                head += part
            status = int(head.split()[1])
            if status != 200:
                outcome = {"outcome": "denied", "status": status}
            else:
                with context.wrap_socket(stream, server_hostname=target) as tls:
                    tls.sendall(f"GET / HTTP/1.1\r\nHost: {target}\r\nConnection: close\r\n\r\n".encode())
                    response = http.client.HTTPResponse(tls)
                    response.begin()
                    body = response.read()
                outcome = {"outcome": "allowed" if body == b"controlled-origin-response\n" else "unexpected", "status": response.status}
        except socket.timeout:
            outcome = {"outcome": "timeout"}
        except OSError as error:
            outcome = {"outcome": "refused", "error": type(error).__name__}
        print(json.dumps({"sequence": sequence, "target": target, "startedAt": started, "finishedAt": time.time(), **outcome}), flush=True)
        sequence += 1
        time.sleep(0.1)
'''


def agent_refusals(logs, since):
    """Times the agent's readiness gate refused a session, from its own log."""
    times = []
    for line in logs.splitlines():
        line = re.sub(r"\x1b\[[0-9;]*m", "", line)
        if "egress session refused" in line:
            stamp = datetime.datetime.fromisoformat(line.split()[0].replace("Z", "+00:00")).timestamp()
            if stamp >= since:
                times.append(stamp)
    return times


def analyze(observations, health, balancer, killed_at, refusals):
    def phase(start, end):
        result = {}
        for target in [ALLOWED, UNLISTED]:
            counts = {}
            for item in observations:
                if item["target"] == target and start <= item["startedAt"] < end:
                    counts[item["outcome"]] = counts.get(item["outcome"], 0) + 1
            result[target] = counts
        successes = sorted(item["finishedAt"] for item in observations
                           if item["target"] == ALLOWED and item["outcome"] == "allowed" and start <= item["startedAt"] < end)
        gaps = [later - earlier for earlier, later in zip(successes, successes[1:])]
        result["maxAllowedSuccessGapSeconds"] = round(max(gaps, default=0), 3)
        return result

    first, last = observations[0]["startedAt"], observations[-1]["finishedAt"]
    # Requests the agent refused at once. A request in flight on the killed replica is also
    # reset at once, so the agent's own log is what proves its readiness gate stayed open.
    gated = [item for item in observations if item["target"] == ALLOWED and item["outcome"] == "refused"
             and item["finishedAt"] - item["startedAt"] < 0.2 and item["startedAt"] >= killed_at]
    closed = [item for item in health if item["startedAt"] >= killed_at and item["state"] not in ("ready", "verifying-egress-path")]
    deregistered = [event["t"] for event in balancer if event.get("event") == "deregistered"]
    return {
        "killedAt": killed_at,
        "deregisteredAfterSeconds": round(deregistered[0] - killed_at, 3) if deregistered else None,
        "beforeKill": phase(first, killed_at),
        "afterKill": phase(killed_at, last + 1),
        "deadReplicaConnections": sum(1 for event in balancer if event.get("event") == "target-failed"),
        "clientFastRefusalsAfterKill": len(gated),
        "agentGateRefusalsAfterKill": len(refusals),
        "agentGateClosedSpanSeconds": round(refusals[-1] - refusals[0], 3) if refusals else 0,
        "agentNotReadyHealthSamplesAfterKill": len(closed),
        "agentNotReadySpanSeconds": round(closed[-1]["startedAt"] - closed[0]["startedAt"], 3) if closed else 0,
        "unlistedAllowed": sum(1 for item in observations if item["target"] == UNLISTED and item["outcome"] in ("allowed", "unexpected")),
    }


def check_replica_failover(suite, image, containers, networks, docker, prefix, output, aud, sub, uid):
    guest = prefix + "-guest"
    docker("network", "create", "--internal", "--subnet", "172.29.91.0/24", guest)
    networks.append(guest)
    (output / "balancer.py").write_text(BALANCER)
    balancer = prefix + "-balancer"
    docker("run", "-d", "--name", balancer, "--network", prefix, "--ip", "93.184.216.10",
           "-v", f"{output / 'balancer.py'}:/balancer.py:ro", "python:3.13-slim",
           "python", "-u", "/balancer.py", ",".join(REPLICAS), str(INTERVAL), str(UNHEALTHY))
    containers.append(balancer)
    docker("network", "connect", "--ip", BALANCER_IP, guest, balancer)
    agent = prefix + "-agent"
    docker("run", "-d", "--name", agent, "--network", guest, "--ip", AGENT_IP, *AGENT_CAP_ARGS,
           "-v", f"{output / 'origin/ca.pem'}:/fixture-ca.pem:ro", image)
    containers.append(agent)

    def call(method, path, body=None, timeout=25):
        if method == "PUT" and path == "/v1/egress":
            body = suite.delivery(body)
        conn = http.client.HTTPConnection(AGENT_IP, 8080, timeout=timeout)
        conn.request(method, path, json.dumps(body) if body is not None else None, {"Content-Type": "application/json"})
        response = conn.getresponse()
        result = response.status, response.read()
        conn.close()
        return result

    deadline = time.monotonic() + 10
    while True:
        try:
            if call("GET", "/v1/health")[0] == 200:
                break
        except OSError:
            pass
        assert time.monotonic() < deadline, "agent did not start"
        time.sleep(0.1)
    hook = {"microvmId": sub, "runHookPayload": json.dumps({"gateway": f"{BALANCER_IP}:3128", "audience": aud})}
    assert call("POST", "/aws/lambda-microvms/runtime/v1/run", hook)[0] == 200
    assert call("PUT", "/v1/egress", {"token": suite.token()})[0] == 204
    status, body = call("GET", "/v1/health")
    assert status == 200 and json.loads(body)["status"] == "ready", (status, body)

    health, stop = [], threading.Event()

    def poll_health():
        while not stop.is_set():
            started = time.time()
            try:
                status, body = call("GET", "/v1/health", timeout=5)
                state = json.loads(body).get("status") if status == 200 else f"http-{status}"
            except (OSError, ValueError) as error:
                state = type(error).__name__
            health.append({"startedAt": started, "state": state})
            time.sleep(0.1)

    poller = threading.Thread(target=poll_health, daemon=True)
    poller.start()
    loop = subprocess.Popen(
        ["docker", "exec", "-u", f"{uid}:{uid}", agent, "python", "-c", LOOP, str(DURATION)],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    time.sleep(KILL_AFTER)
    killed_at = time.time()
    docker("kill", "--signal", "KILL", prefix + "-proxy2")
    lines = loop.communicate(timeout=DURATION + 60)[0].splitlines()
    stop.set()
    poller.join()
    observations = [json.loads(line) for line in lines if line.startswith("{")]
    assert observations, lines
    events = [json.loads(line) for line in docker("logs", balancer).splitlines() if line.startswith("{")]
    (output / "observations.jsonl").write_text("".join(json.dumps(item) + "\n" for item in observations))
    (output / "health.jsonl").write_text("".join(json.dumps(item) + "\n" for item in health))
    (output / "balancer.jsonl").write_text("".join(json.dumps(item) + "\n" for item in events))
    refusals = agent_refusals(docker("logs", agent), killed_at)
    report = analyze(observations, health, events, killed_at, refusals)
    (output / "replica-failover.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2), flush=True)

    def fail_closed():
        assert report["unlistedAllowed"] == 0, report
        assert report["afterKill"][UNLISTED].get("denied", 0) > 0, report
    suite.record("unlisted destination stays denied while a replica dies", fail_closed)

    def stays_open():
        assert report["deadReplicaConnections"] > 0, "no connection reached the killed replica"
        assert report["agentGateRefusalsAfterKill"] == 0, report
        assert report["agentNotReadyHealthSamplesAfterKill"] == 0, report
    suite.record("sandbox egress stays open while the other replica answers", stays_open)

