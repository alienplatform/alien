"""The client pods' verdicts, and the stages they run in."""

import http.server
import tempfile
import threading
import unittest

from loader import probe

MARKER = "marker-for-the-test"


class Listener:
    """Answers HTTP requests until stopped."""

    def __init__(self, serve=True):
        outer = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(MARKER.encode())

            def log_message(self, *args):
                pass

        self.server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
        self.port = self.server.server_address[1]
        if serve:
            threading.Thread(target=self.server.serve_forever, daemon=True).start()
        else:
            self.server.server_close()

    def stop(self):
        self.server.shutdown()
        self.server.server_close()

    def url(self):
        return f"http://127.0.0.1:{self.port}/"


def run(listener, **settings):
    exec(compile(probe.client_code(listener.url(), MARKER), "client", "exec"), dict(settings))


class DeniedClient(unittest.TestCase):
    def test_a_denied_client_that_is_refused_passes(self):
        run(Listener(serve=False), DENIED=True)

    def test_a_denied_client_that_reaches_the_listener_fails(self):
        listener = Listener()
        try:
            with self.assertRaisesRegex(RuntimeError, "NetworkPolicy did not block traffic"):
                run(listener, DENIED=True)
        finally:
            listener.stop()


class ControlClient(unittest.TestCase):
    def test_control_reaches_the_listener(self):
        listener = Listener()
        try:
            run(listener, DENIED=False)
        finally:
            listener.stop()

    def test_control_that_is_refused_fails(self):
        with self.assertRaises(RuntimeError):
            run(Listener(serve=False), DENIED=False)


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
        # Two overlapping stages: pod starts cost two, not five, one after the other.
        self.assertEqual(len(stages), 2)

    def test_every_probe_role_still_runs(self):
        roles = [role for stage in probe.STAGES for role, _, _ in stage]
        self.assertEqual(
            sorted(roles),
            sorted(["control", "denied", "control-after", "ingress-denied", "ingress-control-after"]),
        )
        denied = {role for stage in probe.STAGES for role, d, _ in stage if d}
        self.assertEqual(denied, {"denied", "ingress-denied"})

    def test_a_control_runs_in_the_stage_after_every_denied_client(self):
        for index, stage in enumerate(probe.STAGES[:-1]):
            self.assertTrue(any(d for _, d, _ in stage))
            self.assertTrue(any(not d for _, d, _ in probe.STAGES[index + 1]))
        self.assertTrue(any(not d for _, d, _ in probe.STAGES[0]))


if __name__ == "__main__":
    unittest.main()
