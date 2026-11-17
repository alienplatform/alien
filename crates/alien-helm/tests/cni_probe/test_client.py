"""The client pods: main's verdicts hold, and each pod leaves a one-line verdict for the runner."""

import http.server
import pathlib
import socket
import subprocess
import sys
import tempfile
import threading
import unittest

from loader import probe

MARKER = "marker-for-the-test"


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Listener:
    def __init__(self, port=None):
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(MARKER.encode())

            def log_message(self, *args):
                pass

        self.port = port or free_port()
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", self.port), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def stop(self):
        self.server.shutdown()
        self.server.server_close()


def run_client(port, denied=False, recover=0):
    """Run the client as a pod does (`python3 -c`); returns (exit code, termination message)."""
    log = pathlib.Path(tempfile.mkdtemp()) / "termination-log"
    code = (f"DENIED={denied!r}\nRECOVER={recover!r}\nLOG={str(log)!r}\n"
            + probe.client_code(f"http://127.0.0.1:{port}/", MARKER))
    done = subprocess.run([sys.executable, "-c", code], capture_output=True, timeout=60)
    return done.returncode, log.read_text() if log.exists() else None


class Verdicts(unittest.TestCase):
    def test_a_control_that_reaches_the_listener_exits_0_with_a_verdict(self):
        listener = Listener()
        try:
            code, message = run_client(listener.port)
        finally:
            listener.stop()
        self.assertEqual(code, 0)
        self.assertEqual(message, "reached the listener on 3 of 3 attempts")

    def test_a_denied_pod_that_is_refused_exits_0_with_a_verdict(self):
        code, message = run_client(free_port(), denied=True)  # nothing listens: refused
        self.assertEqual(code, 0)
        self.assertEqual(message, "refused on all 3 attempts")

    def test_a_denied_pod_that_reaches_the_listener_fails_with_its_reason(self):
        listener = Listener()
        try:
            code, message = run_client(listener.port, denied=True)
        finally:
            listener.stop()
        self.assertNotEqual(code, 0)
        self.assertEqual(message, "FAILED: NetworkPolicy did not block traffic")

    def test_a_control_that_is_refused_fails_with_its_reason(self):
        code, message = run_client(free_port())
        self.assertNotEqual(code, 0)
        self.assertTrue(message.startswith("FAILED: "), message)

    def test_a_recovering_control_waits_for_the_listener(self):
        port = free_port()
        holder = {}
        timer = threading.Timer(2, lambda: holder.update(listener=Listener(port)))
        timer.start()
        try:
            code, message = run_client(port, recover=30)
        finally:
            timer.join()
            holder["listener"].stop()
        self.assertEqual(code, 0)
        self.assertIn("reached the listener", message)


class Pods(unittest.TestCase):
    source = pathlib.Path(probe.__file__).read_text()

    def test_a_pod_has_enough_memory_to_start_under_a_vm_runtime(self):
        # 64Mi never started on AKS (Kata); 128Mi does.
        self.assertGreaterEqual(int(probe.CLIENT_MEMORY_LIMIT.removesuffix("Mi")), 128)
        self.assertIn('"memory": CLIENT_MEMORY_LIMIT', self.source)
        self.assertNotIn('"64Mi"', self.source)

    def test_the_clients_of_a_stage_start_together(self):
        self.assertEqual([[role for role, _, _ in stage] for stage in probe.STAGES],
                         [["control", "denied", "ingress-denied"],
                          ["control-after", "ingress-control-after"]])

    def test_the_verdict_comes_from_the_termination_message_not_the_logs(self):
        self.assertNotIn("FallbackToLogsOnError", self.source)


if __name__ == "__main__":
    unittest.main()
