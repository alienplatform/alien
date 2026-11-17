"""The real client code (`client_code`) run as separate processes against real HTTP servers.

Unit tests with fakes passed addresses the real stages never produce. Here the stages
(`run_stages`) build the control's targets themselves, as the install does, and every client is
a `python3 -c` process, as a pod's container is. Loopback addresses stand in for pod addresses.
"""

import http.server
import socket
import subprocess
import sys
import threading
import time
import unittest

from loader import probe

MARKER = "e2e-marker"
FAST = {"WINDOW": 3, "HOLD": 2, "SERVE_WAIT": 10, "CONTROL_RUN": 12}
# The runner's wait for a denied pod is capped in these tests (the real bound is the control's run).
BOUND_CAP = 8


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Stub:
    """A server answering the marker on its port."""

    def __init__(self, port):
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(MARKER.encode())

            def log_message(self, *args):
                pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def stop(self):
        self.server.shutdown()
        self.server.server_close()


def stamp(seconds):
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(seconds))


class Run:
    """The stages of one install run with every client a process (`python3 -c`, as in a pod).

    `eight_thousand_and_eighty[role]` is the port a denied pod's refusal check goes to: a live
    port stands for "not refused", a closed one for the policy's refusal. `settings[role]`
    overrides the client's constants. `wait` answers as the runner does: a pod status with the
    exit code, the termination message and the times, or an error naming the pod.
    """

    def __init__(self, live, allowed, refused_on, settings=None):
        self.live = live
        self.allowed = allowed
        self.refused_on = refused_on
        self.settings = settings or {}
        self.processes = {}
        self.began = {}
        self.ended = {}

    def code(self, role, listener_port):
        return probe.client_code(
            f"http://127.0.0.1:{listener_port}/", MARKER, f"http://127.0.0.1:{self.allowed}/")

    def start(self, role, **flags):
        settings = {**FAST, **self.settings.get(role, {}), **flags}
        header = "".join(f"{name}={value!r}\n" for name, value in settings.items()) + "LOG='/dev/null'\n"
        port = self.refused_on[role] if role in probe.DENIED_ROLES else self.live
        process = subprocess.Popen([sys.executable, "-B", "-c", header + self.code(role, port)],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.processes[role] = process
        self.began[role] = time.time()
        def reap():
            process.wait()
            self.ended[role] = time.time()
        threading.Thread(target=reap, daemon=True).start()

    def running(self, role):
        return {"status": {"podIP": "127.0.0.1"}}

    def pod(self, role):
        process = self.processes[role]
        text = process.stdout.read() + process.stderr.read()
        lines = [line for line in text.splitlines() if line]
        message = next((line for line in reversed(lines) if line.startswith(("FAILED", "blocked", "listener", "reached"))), "")
        return {"status": {"phase": "Succeeded" if process.returncode == 0 else "Failed",
                           "containerStatuses": [{"state": {"terminated": {
                               "exitCode": process.returncode, "message": message,
                               "startedAt": stamp(self.began[role]),
                               "finishedAt": stamp(self.ended[role])}}}]}}

    def wait(self, role, bound=None, unfinished=None):
        process = self.processes[role]
        try:
            process.wait(timeout=90 if bound is None else min(bound, BOUND_CAP))
        except subprocess.TimeoutExpired:
            raise RuntimeError(unfinished or f"{role} did not finish")
        time.sleep(1.1)  # the reaper stamps the end
        pod = self.pod(role)
        if process.returncode != 0:
            raise RuntimeError(f"{role} probe failed: "
                               + pod["status"]["containerStatuses"][0]["state"]["terminated"]["message"])
        return pod

    def run(self):
        probe.run_stages(self.start, self.wait, self.running)
        return self


class EndToEnd(unittest.TestCase):
    def setUp(self):
        self.stubs = []
        self.run_ = None

    def tearDown(self):
        for stub in self.stubs:
            stub.stop()
        if self.run_:
            for process in self.run_.processes.values():
                process.kill()
                process.wait()
                process.stdout.close()
                process.stderr.close()

    def serve(self, port):
        self.stubs.append(Stub(port))

    def stages(self, refused_on, settings=None):
        """`refused_on` maps each denied role to the port its 8080 check goes to."""
        live, allowed = free_port(), free_port()
        self.serve(live)
        self.serve(allowed)
        refused_on = {role: (live if port == "live" else free_port()) for role, port in refused_on.items()}
        self.run_ = Run(live, allowed, refused_on, settings)
        return self.run_

    def fails(self, run, pattern):
        with self.assertRaisesRegex(RuntimeError, pattern):
            run.run()

    def test_enforcement_passes(self):
        run = self.stages({"denied": "closed", "ingress-denied": "closed"}).run()
        for role, process in run.processes.items():
            self.assertEqual(process.returncode, 0, role)

    def test_no_enforcement_fails(self):
        self.fails(self.stages({"denied": "live", "ingress-denied": "live"}),
                   "NetworkPolicy did not block traffic")

    def test_egress_only_enforcement_fails(self):
        # The egress policy refuses the denied pod; nothing refuses the ingress-denied one.
        self.fails(self.stages({"denied": "closed", "ingress-denied": "live"}),
                   "ingress-denied probe failed: FAILED: NetworkPolicy did not block traffic")

    def test_ingress_only_enforcement_fails(self):
        self.fails(self.stages({"denied": "live", "ingress-denied": "closed"}),
                   "denied probe failed: FAILED: NetworkPolicy did not block traffic")

    def test_a_denied_pod_that_never_finishes_fails_naming_it(self):
        # It counts for far longer than the runner waits.
        run = self.stages({"denied": "live", "ingress-denied": "closed"},
                          {"denied": {"WINDOW": 1000}})
        self.fails(run, "the denied pod did not finish before the control did")

    def test_a_control_that_ends_before_a_denied_pod_fails(self):
        # The denied pods hold for 7 s; the control runs for 3 s.
        run = self.stages({"denied": "closed", "ingress-denied": "closed"},
                          {"control": {"CONTROL_RUN": 3}, "denied": {"HOLD": 7}, "ingress-denied": {"HOLD": 7}})
        self.fails(run, "control finished before the denied pod")

    def test_a_listener_outage_the_control_sees_fails(self):
        run = self.stages({"denied": "closed", "ingress-denied": "closed"}, {"control": {"CONTROL_RUN": 30}})
        threading.Timer(3, self.stubs[0].stop).start()
        self.fails(run, "control probe failed: FAILED: (listener unreachable from the control|<?Connection)|probe failed")

    def test_the_allowed_port_never_reachable_fails_with_the_network_not_up_message(self):
        run = self.stages({"denied": "closed", "ingress-denied": "closed"}, {"denied": {"SERVE_WAIT": 2}})
        self.stubs[1].stop()  # the allowed port
        self.fails(run, "denied probe failed: FAILED: allowed port never reachable: its own network is not up")

    def test_the_allowed_port_answering_late_passes(self):
        # 8081 is unreachable for the first seconds, then fine: refusals before it are not counted.
        run = self.stages({"denied": "closed", "ingress-denied": "closed"},
                          {"denied": {"SERVE_WAIT": 10}, "ingress-denied": {"SERVE_WAIT": 10},
                           "control": {"CONTROL_RUN": 16}})
        self.stubs[1].stop()
        port = run.allowed
        threading.Timer(3, lambda: self.serve(port)).start()
        run.run()
        for role, process in run.processes.items():
            self.assertEqual(process.returncode, 0, role)


if __name__ == "__main__":
    unittest.main()
