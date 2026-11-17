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
import unittest

from loader import probe

MARKER = "e2e-marker"
FAST = "WINDOW=3\nHOLD=2\nLINGER=4\nSERVED_WAIT=15\nSERVE_WAIT=10\nCONTROL_RUN=40\nLOG='/dev/null'\n"
DENIED_ADDRESSES = {"denied": "127.0.0.2", "ingress-denied": "127.0.0.3"}


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


class Run:
    """The stages of one install run with every client a process. Results by role."""

    def __init__(self, denied_listener_port, allowed_port, live_port):
        self.denied_code = probe.client_code(
            f"http://127.0.0.1:{denied_listener_port}/", MARKER, f"http://127.0.0.1:{allowed_port}/")
        self.plain_code = probe.client_code(
            f"http://127.0.0.1:{live_port}/", MARKER, f"http://127.0.0.1:{allowed_port}/")
        self.processes = {}
        self.targets = None

    def start(self, role, **settings):
        denied = role in probe.DENIED_ROLES
        header = FAST + "".join(f"{name}={value!r}\n" for name, value in settings.items())
        if denied:
            header += f"BIND={DENIED_ADDRESSES[role]!r}\n"
        else:
            self.targets = settings.get("TARGETS", self.targets)
        code = header + (self.denied_code if denied else self.plain_code)
        self.processes[role] = subprocess.Popen(
            [sys.executable, "-B", "-c", code], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)

    def running(self, role):
        return {"status": {"podIP": DENIED_ADDRESSES[role]}}

    def wait(self, role):
        process = self.processes[role]
        process.wait(timeout=90)

    def run(self):
        probe.run_stages(self.start, self.wait, self.running)
        return self

    def output(self, role):
        process = self.processes[role]
        return process.returncode, process.stdout.read() + process.stderr.read()


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

    def serve(self, port):
        self.stubs.append(Stub(port))

    def stages(self, denied_listener_port, allowed_port, live_port):
        run = self.run_ = Run(denied_listener_port, allowed_port, live_port)
        probe.run_stages(run.start, run.wait, run.running)
        return run

    def test_a_pass_with_the_denied_8080_refused_and_the_allowed_port_answering(self):
        live, allowed, closed = free_port(), free_port(), free_port()
        self.serve(live)
        self.serve(allowed)
        run = self.stages(closed, allowed, live)  # `closed` stands for the policy's refusal
        for role in run.processes:
            code, text = run.output(role)
            self.assertEqual(code, 0, f"{role}: {text}")

    def test_the_control_reaches_each_denied_pods_state(self):
        live, allowed, closed = free_port(), free_port(), free_port()
        self.serve(live)
        self.serve(allowed)
        run = self.stages(closed, allowed, live)
        # The control gets whole URLs from the stages and exits 0 only after reading ` done` from
        # `<url>state` of both denied pods.
        self.assertEqual(run.targets, [f"http://{ip}:8080/" for ip in DENIED_ADDRESSES.values()])
        code, text = run.output("control")
        self.assertEqual(code, 0, text)
        self.assertIn("2 denied pods finished", text)

    def test_everything_allowed_is_a_failure(self):
        live, allowed = free_port(), free_port()
        self.serve(live)
        self.serve(allowed)
        run = self.stages(live, allowed, live)  # no enforcement: the denied pods reach 8080 too
        code, text = run.output("denied")
        self.assertNotEqual(code, 0)
        self.assertIn("NetworkPolicy did not block traffic", text)

    def test_both_ports_dropped_is_a_failure(self):
        live, allowed, closed = free_port(), free_port(), free_port()
        self.serve(live)  # the control reaches the listener; nothing listens on `allowed`
        run = self.stages(closed, allowed, live)
        code, text = run.output("denied")
        self.assertNotEqual(code, 0)
        self.assertIn("cannot reach the listener on the allowed port", text)


if __name__ == "__main__":
    unittest.main()
