"""The CNI probe's client pods against a listener whose policy takes effect late, never, or flaps."""

import http.server
import importlib.util
import pathlib
import socket
import tempfile
import threading
import unittest

SOURCE = pathlib.Path(__file__).parents[2] / "src" / "emitters" / "sandbox_cni_probe.py"
spec = importlib.util.spec_from_file_location("sandbox_cni_probe", SOURCE)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

MARKER = "marker-for-the-test"


class Listener:
    """Answers `reachable` requests, then stops listening (connections are refused)."""

    def __init__(self, reachable, then_reachable_again=0):
        self.remaining = reachable
        self.server = None
        self.port = None
        self.lock = threading.Lock()
        self.reopen_after = then_reachable_again
        self.start()

    def start(self):
        outer = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(MARKER.encode())
                with outer.lock:
                    outer.remaining -= 1
                    done = outer.remaining <= 0
                if done:
                    threading.Thread(target=outer.stop).start()

            def log_message(self, *args):
                pass

        self.server = http.server.HTTPServer(("127.0.0.1", self.port or 0), Handler)
        self.port = self.server.server_address[1]
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def stop(self):
        self.server.shutdown()
        self.server.server_close()

    def url(self):
        return f"http://127.0.0.1:{self.port}/"


def run(listener, **settings):
    log = tempfile.NamedTemporaryFile("r", suffix=".log")
    code = probe.client_code(listener.url(), MARKER)
    scope = {"LOG": log.name, "WINDOW": 20, **settings}
    exec(compile(code, "client", "exec"), scope)
    return open(log.name).read()


class DeniedClient(unittest.TestCase):
    def test_policy_that_applies_after_a_few_requests_passes(self):
        listener = Listener(reachable=3)
        note = run(listener, DENIED=True)
        self.assertIn("blocked after", note)
        self.assertIn("3 reachable attempts first", note)

    def test_policy_that_blocks_from_the_start_passes(self):
        listener = Listener(reachable=0)
        listener.stop()
        self.assertIn("0 reachable attempts first", run(listener, DENIED=True))

    def test_policy_that_never_takes_effect_fails(self):
        listener = Listener(reachable=10**6)
        try:
            with self.assertRaisesRegex(RuntimeError, "NetworkPolicy did not block traffic"):
                run(listener, DENIED=True, WINDOW=3)
        finally:
            listener.stop()

    def test_traffic_that_resumes_after_a_refusal_fails(self):
        listener = Listener(reachable=0)
        listener.stop()

        def reopen():
            threading.Event().wait(1.5)
            listener.remaining = 10**6
            listener.start()

        threading.Thread(target=reopen, daemon=True).start()
        try:
            with self.assertRaisesRegex(RuntimeError, "reachable again"):
                run(listener, DENIED=True)
        finally:
            listener.stop()


class ControlClient(unittest.TestCase):
    def test_control_reaches_the_listener(self):
        listener = Listener(reachable=10**6)
        try:
            run(listener, DENIED=False)
        finally:
            listener.stop()

    def test_control_that_is_refused_fails(self):
        listener = Listener(reachable=0)
        listener.stop()
        with self.assertRaises(RuntimeError):
            run(listener, DENIED=False)


class Stages(unittest.TestCase):
    def test_clients_of_a_stage_start_together_before_any_is_awaited(self):
        events = []
        probe.run_stages(
            lambda role, denied, recover: events.append(("start", role)),
            lambda role: events.append(("wait", role)),
        )
        stages = [[role for role, _, _ in stage] for stage in probe.STAGES]
        position = 0
        for stage in stages:
            started = [role for kind, role in events[position:position + len(stage)] if kind == "start"]
            self.assertEqual(started, stage)
            position += len(stage)
            awaited = [role for kind, role in events[position:position + len(stage)] if kind == "wait"]
            self.assertEqual(awaited, stage)
            position += len(stage)
        # Two overlapping stages: pod starts cost three, not five, one after the other.
        self.assertEqual(len(stages), 2)

    def test_every_probe_role_still_runs(self):
        roles = [role for stage in probe.STAGES for role, _, _ in stage]
        self.assertEqual(
            sorted(roles),
            sorted(["control", "denied", "control-after", "ingress-denied", "ingress-control-after"]),
        )
        denied = {role for stage in probe.STAGES for role, d, _ in stage if d}
        self.assertEqual(denied, {"denied", "ingress-denied"})


if __name__ == "__main__":
    unittest.main()
