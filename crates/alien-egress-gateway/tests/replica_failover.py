"""Kill one gateway replica behind a load balancer while a sandbox sends steady traffic.

Every new connection goes to a random registered replica, and a replica is removed only after
UNHEALTHY consecutive failed health checks INTERVAL seconds apart. Until then, connections sent
to the killed replica fail. Any one sandbox policy check can land there, but egress must stay
open while the other replica answers, and unlisted destinations must stay denied throughout.

Two balancer models:
- "passthrough" (default) models a network load balancer such as an AWS NLB or a Kubernetes
  Service: it rewrites packets and does not terminate TCP, so a connection routed to the killed
  replica never completes its handshake. The agent must reopen such forwarded connections on a
  new connection before the client gives up.
- "proxy" terminates TCP and dials the replica afterwards, so a connection routed to the killed
  replica is accepted and then closed without an answer when the dial gives up.
"""
import datetime
import json
import os
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
KILLED = "93.184.216.4"
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

PASSTHROUGH_IMAGE = r'''
FROM python:3.13-slim
RUN apt-get update && apt-get install -y --no-install-recommends iptables && rm -rf /var/lib/apt/lists/*
'''

PASSTHROUGH = r'''
import json, re, socket, subprocess, sys, threading, time
targets = sys.argv[1].split(",")
interval, unhealthy, address = float(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
registered, failures, lock = set(targets), {target: 0 for target in targets}, threading.Lock()
output = threading.Lock()

def log(**event):
    with output:
        print(json.dumps({"t": time.time(), **event}), flush=True)

def iptables(*args):
    return subprocess.run(["iptables", "-w", *args], check=True, capture_output=True, text=True).stdout

def routed():
    # The nat table sees only the first packet of a connection, so each DNAT rule's packet
    # counter is the number of connections routed to that target.
    listing = iptables("-t", "nat", "-L", "BALANCE", "-v", "-x", "-n")
    return {match.group(2): int(match.group(1)) for match in re.finditer(r"^\s*(\d+).*to:(\S+)", listing, re.M)}

def route():
    # Replace the whole rule set in one transaction; each new connection picks a registered
    # target uniformly, and established connections keep their target.
    live, rules = sorted(registered), ["*nat", ":BALANCE - [0:0]"]
    for index, target in enumerate(live):
        remaining = len(live) - index
        pick = f"-m statistic --mode random --probability {1 / remaining:.6f} " if remaining > 1 else ""
        rules.append(f"-A BALANCE -p tcp {pick}-j DNAT --to-destination {target}")
    rules.append("COMMIT")
    subprocess.run(["iptables-restore", "-w", "--noflush"], input="\n".join(rules) + "\n", text=True, check=True)

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
            failures[target] = 0 if healthy else failures[target] + 1
            if failures[target] >= unhealthy and target in registered:
                with lock:
                    log(event="routed", counts=routed())
                    registered.discard(target)
                    route()
                log(event="deregistered", target=target)

iptables("-t", "nat", "-N", "BALANCE")
route()
iptables("-t", "nat", "-A", "PREROUTING", "-d", address, "-p", "tcp", "--dport", "3128", "-j", "BALANCE")
iptables("-t", "nat", "-A", "POSTROUTING", "-p", "tcp", "--dport", "3128", "-j", "MASQUERADE")
threading.Thread(target=health, daemon=True).start()
log(event="listening", targets=targets)
while True:
    time.sleep(0.2)
    with lock:
        log(event="routed", counts=routed())
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


def agent_retries(logs, since):
    """Forwarded connections the agent reopened because no gateway answered an attempt."""
    count = 0
    for line in logs.splitlines():
        line = re.sub(r"\x1b\[[0-9;]*m", "", line)
        if "gateway connection unanswered; retrying" in line:
            stamp = datetime.datetime.fromisoformat(line.split()[0].replace("Z", "+00:00")).timestamp()
            count += stamp >= since
    return count


def analyze(observations, health, balancer, killed_at, refusals, retries):
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
    killed = KILLED + ":3128"
    routed = [(event["t"], event["counts"][killed]) for event in balancer
              if event.get("event") == "routed" and killed in event["counts"]]
    before = [count for stamp, count in routed if stamp <= killed_at]
    routed_to_killed = max((count for _, count in routed), default=0) - (before[-1] if before else 0)
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
        "allowedAfterDeregistration": sum(1 for item in observations if deregistered and item["target"] == ALLOWED
                                          and item["outcome"] == "allowed" and item["startedAt"] >= deregistered[0]),
        # Proxy model: dials the balancer gave up on. Pass-through model: connections routed
        # to the killed replica after the kill.
        "deadReplicaConnections": sum(1 for event in balancer if event.get("event") == "target-failed") + routed_to_killed,
        # Requests started after the kill that got no gateway answer: a client timeout, or the
        # agent closing the connection when it gave up on the gateway.
        "clientFailuresAfterKill": sum(1 for item in observations if item["startedAt"] >= killed_at
                                       and item["outcome"] in ("timeout", "refused")),
        "clientTimeoutsAfterKill": sum(1 for item in observations if item["outcome"] == "timeout" and item["startedAt"] >= killed_at),
        "slowestRequestAfterKillSeconds": round(max((item["finishedAt"] - item["startedAt"] for item in observations
                                                     if item["startedAt"] >= killed_at), default=0), 3),
        "agentForwardRetriesAfterKill": retries,
        "clientFastRefusalsAfterKill": len(gated),
        "agentGateRefusalsAfterKill": len(refusals),
        "agentGateClosedSpanSeconds": round(refusals[-1] - refusals[0], 3) if refusals else 0,
        "agentNotReadyHealthSamplesAfterKill": len(closed),
        "agentNotReadySpanSeconds": round(closed[-1]["startedAt"] - closed[0]["startedAt"], 3) if closed else 0,
        "unlistedAllowed": sum(1 for item in observations if item["target"] == UNLISTED and item["outcome"] in ("allowed", "unexpected")),
    }


def check_replica_failover(suite, image, containers, networks, docker, prefix, output, aud, sub, uid, model="passthrough"):
    guest = prefix + "-guest"
    docker("network", "create", "--internal", "--subnet", "172.29.91.0/24", guest)
    networks.append(guest)
    balancer = prefix + "-balancer"
    if model == "passthrough":
        balancer_image = prefix + "-passthrough-balancer"
        subprocess.run(["docker", "build", "-q", "-t", balancer_image, "-"], input=PASSTHROUGH_IMAGE, text=True, check=True,
                       env={**os.environ, "DOCKER_HOST": "unix:///var/run/docker.sock"}, stdout=subprocess.DEVNULL)
        (output / "balancer.py").write_text(PASSTHROUGH)
        options = ["--cap-add", "NET_ADMIN", "--sysctl", "net.ipv4.ip_forward=1"]
    else:
        balancer_image = "python:3.13-slim"
        (output / "balancer.py").write_text(BALANCER)
        options = []
    # Attach both networks before the balancer starts: the pass-through rules name its guest address.
    docker("create", "--name", balancer, "--network", prefix, "--ip", "93.184.216.10", *options,
           "-v", f"{output / 'balancer.py'}:/balancer.py:ro", balancer_image,
           "python", "-u", "/balancer.py", ",".join(REPLICAS), str(INTERVAL), str(UNHEALTHY), BALANCER_IP)
    containers.append(balancer)
    docker("network", "connect", "--ip", BALANCER_IP, guest, balancer)
    docker("start", balancer)
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
    assert loop.returncode == 0, lines[-20:]
    stop.set()
    poller.join()
    observations = [json.loads(line) for line in lines if line.startswith("{")]
    assert observations, lines
    events = [json.loads(line) for line in docker("logs", balancer).splitlines() if line.startswith("{")]
    (output / "observations.jsonl").write_text("".join(json.dumps(item) + "\n" for item in observations))
    (output / "health.jsonl").write_text("".join(json.dumps(item) + "\n" for item in health))
    (output / "balancer.jsonl").write_text("".join(json.dumps(item) + "\n" for item in events))
    agent_logs = docker("logs", agent)
    report = analyze(observations, health, events, killed_at, agent_refusals(agent_logs, killed_at),
                     agent_retries(agent_logs, killed_at))
    report["balancer"] = model
    (output / "replica-failover.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2), flush=True)

    def fail_closed():
        assert report["unlistedAllowed"] == 0, report
        assert report["afterKill"][UNLISTED].get("denied", 0) > 0, report
    suite.record("unlisted destination stays denied while a replica dies", fail_closed)

    def stays_open():
        assert report["deadReplicaConnections"] > 0, "no connection reached the killed replica"
        assert report["afterKill"][ALLOWED].get("allowed", 0) > 0, report
        assert report["deregisteredAfterSeconds"] is not None, report
        assert report["allowedAfterDeregistration"] > 0, report
        assert report["agentGateRefusalsAfterKill"] == 0, report
        assert report["agentNotReadyHealthSamplesAfterKill"] == 0, report
    suite.record("sandbox egress stays open while the other replica answers", stays_open)

    if model == "passthrough":
        def reopened():
            assert report["clientFailuresAfterKill"] == 0, report
        suite.record("forwarded connections routed to the dead replica are reopened", reopened)
