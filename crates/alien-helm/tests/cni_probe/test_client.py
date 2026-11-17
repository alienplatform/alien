"""The client pods' verdicts: refusals count only while the listener and the denied pod are up."""

import http.server
import pathlib
import socket
import threading
import time
import unittest

from loader import probe

MARKER = "marker-for-the-test"
FAST = dict(WINDOW=4, HOLD=2, SERVE_WAIT=3, CONTROL_RUN=20)


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Listener:
    """Answers HTTP requests with the marker until stopped (`url` of a stopped one is refused)."""

    def __init__(self, serve=True, port=None, allowed=True):
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(MARKER.encode())

            def log_message(self, *args):
                pass

        self.port = port or free_port()
        self.allowed_port = free_port()
        # The listener's second port, the one the denied pods' policies admit.
        self.servers = []
        if serve:
            self.servers.append(http.server.ThreadingHTTPServer(("127.0.0.1", self.port), Handler))
        if allowed:
            self.servers.append(http.server.ThreadingHTTPServer(("127.0.0.1", self.allowed_port), Handler))
        for server in self.servers:
            threading.Thread(target=server.serve_forever, daemon=True).start()

    def stop(self):
        for server in self.servers:
            server.shutdown()
            server.server_close()
        self.servers = []

    def allowed_url(self):
        return f"http://127.0.0.1:{self.allowed_port}/"

    def url(self):
        return f"http://127.0.0.1:{self.port}/"


class Pod:
    """A client pod's code run in a thread, with its outcome."""

    def __init__(self, listener, **settings):
        self.error = None
        self.log = None
        code = probe.client_code(listener.url(), MARKER, listener.allowed_url())
        scope = {"LOG": "/dev/null", **FAST, **settings}
        self.thread = threading.Thread(target=self._run, args=(code, scope), daemon=True)
        self.thread.start()

    def _run(self, code, scope):
        try:
            exec(compile(code, "client", "exec"), scope)
        except BaseException as error:  # the pod's exit status
            self.error = error

    def result(self, timeout=30):
        self.thread.join(timeout)
        assert not self.thread.is_alive(), "the client did not finish"
        return self.error


def denied(listener, **settings):
    return Pod(listener, DENIED=True, **settings)


class DeniedPodsAndControl(unittest.TestCase):
    def test_refusals_with_a_listener_that_stays_up_pass(self):
        up = Listener()
        refused = Listener(serve=False)  # what enforcement does to the denied pods
        try:
            control = Pod(up, CONTROL=True, CONTROL_RUN=8)
            first, second = denied(refused), denied(refused)
            self.assertIsNone(first.result())
            self.assertIsNone(second.result())
            self.assertIsNone(control.result())
        finally:
            up.stop()

    def test_a_listener_outage_fails_the_control(self):
        listener = Listener()
        control = Pod(listener, CONTROL=True, CONTROL_RUN=20)
        time.sleep(2)
        listener.stop()
        # Either the refusal is named, or the connection cut off by the stop is reported: a failure.
        self.assertRegex(str(control.result()), "listener unreachable from the control|ConnectionReset|RemoteDisconnected")

    def test_a_listener_outage_partway_with_no_enforcement_fails_the_denied_pod_too(self):
        listener = Listener()
        pod = denied(listener, WINDOW=8, HOLD=3)
        time.sleep(2)
        listener.stop()
        # The allowed port goes too: its refusals prove nothing (a connection cut off by the stop
        # is a failure as well).
        self.assertRegex(str(pod.result()), "cannot reach the listener on the allowed port|ConnectionReset|RemoteDisconnected")

    def test_traffic_that_never_stops_fails_the_denied_pod(self):
        up = Listener()
        try:
            self.assertRegex(str(denied(up).result()), "NetworkPolicy did not block traffic")
        finally:
            up.stop()

    def test_traffic_that_resumes_after_a_refusal_fails_the_denied_pod(self):
        port = free_port()
        flapping = Listener(serve=False, port=port)
        pod = denied(flapping, HOLD=4)
        time.sleep(1.5)
        flapping = Listener(port=port)
        try:
            self.assertRegex(str(pod.result()), "reachable again")
        finally:
            flapping.stop()


class AllowedPort(unittest.TestCase):
    """The denied pod's own egress must work on the allowed port while 8080 is refused."""

    def test_a_denied_pod_whose_allowed_port_never_answers_fails_naming_its_own_network(self):
        # Egress dropped by something else: both ports fail, so the refusal proves nothing.
        both_down = Listener(serve=False, allowed=False)
        self.assertRegex(str(denied(both_down, SERVE_WAIT=2).result()),
                         "allowed port never reachable: its own network is not up")

    def test_a_late_first_answer_on_the_allowed_port_is_waited_for(self):
        # The policies land a moment after the pod starts: 8081 answers only after a few seconds.
        # Refusals of 8080 before that are not counted.
        listener = Listener(serve=False, allowed=False)
        pod = denied(listener, SERVE_WAIT=10)
        time.sleep(3)
        late = http.server.ThreadingHTTPServer(("127.0.0.1", listener.allowed_port), type(
            "H", (http.server.BaseHTTPRequestHandler,), {
                "do_GET": lambda self: (self.send_response(200), self.end_headers(), self.wfile.write(MARKER.encode())),
                "log_message": lambda self, *a: None}))
        threading.Thread(target=late.serve_forever, daemon=True).start()
        try:
            self.assertIsNone(pod.result())
        finally:
            late.shutdown()
            late.server_close()

    def test_the_allowed_port_is_checked_on_every_tick_of_the_hold(self):
        # 8080 is refused and 8081 answers at first; 8081 goes away during the hold.
        listener = Listener(serve=False)
        pod = denied(listener, WINDOW=8, HOLD=6)
        time.sleep(3)
        listener.stop()
        self.assertRegex(str(pod.result()), "cannot reach the listener on the allowed port|ConnectionReset|RemoteDisconnected")

    def test_the_client_names_the_same_address_on_two_ports(self):
        code = probe.client_code("http://10.1.2.3:8080/", MARKER, "http://10.1.2.3:8081/")
        self.assertIn("http://10.1.2.3:8081/", code)
        self.assertEqual(probe.pod_url("fd00::1", 8081), "http://[fd00::1]:8081/")


class Verdicts(unittest.TestCase):
    """Each client writes its one-line verdict where the kubelet reads the termination message."""

    def run_with_log(self, listener, **settings):
        import tempfile
        with tempfile.NamedTemporaryFile("r") as log:
            pod = Pod(listener, **{**settings, "LOG": log.name})
            error = pod.result()
            return error, log.read()

    def test_a_pass_and_a_failure_each_leave_a_line(self):
        up = Listener()
        try:
            error, line = self.run_with_log(up)
            self.assertIsNone(error)
            self.assertIn("reached the listener", line)
            error, line = self.run_with_log(Listener(serve=False, allowed=False), DENIED=True, SERVE_WAIT=1)
            self.assertIsNotNone(error)
            self.assertEqual(line, "FAILED: allowed port never reachable: its own network is not up")
        finally:
            up.stop()

    def test_the_pod_spec_reads_the_message_from_the_file(self):
        self.assertIn('"terminationMessagePolicy": "File"', pathlib.Path(probe.__file__).read_text())


class PlainControls(unittest.TestCase):
    def run_code(self, listener, **settings):
        return Pod(listener, **settings).result()

    def test_control_reaches_the_listener(self):
        up = Listener()
        try:
            self.assertIsNone(self.run_code(up))
        finally:
            up.stop()

    def test_control_that_is_refused_fails(self):
        self.assertRegex(str(self.run_code(Listener(serve=False))), "positive control was not reached")

    def test_a_recovering_control_waits_for_the_policy_to_go(self):
        port = free_port()
        down = Listener(serve=False, port=port)
        pod = Pod(down, RECOVER=6)
        time.sleep(1.5)
        up = Listener(port=port)
        try:
            self.assertIsNone(pod.result())
        finally:
            up.stop()

    def test_a_recovering_control_that_is_never_reached_fails(self):
        self.assertRegex(str(self.run_code(Listener(serve=False), RECOVER=2)), "traffic did not resume")


class Stages(unittest.TestCase):
    def run_all(self):
        events = []

        def start(role, **settings):
            events.append(("start", role, settings))

        def wait(role, bound=None, unfinished=None):
            events.append(("wait", role, bound))
            return passed(role)

        def running(role):
            events.append(("running", role))
            return {"status": {"podIP": "10.0.0." + str(len(events))}}

        probe.run_stages(start, wait, running)
        return events

    def test_the_control_starts_first_and_the_denied_pods_follow_it(self):
        events = self.run_all()
        kinds = [(e[0], e[1]) for e in events]
        self.assertEqual(kinds[:4], [
            ("start", "control"), ("running", "control"), ("start", "denied"), ("start", "ingress-denied")])
        self.assertEqual(events[0][2], {"CONTROL": True})
        self.assertEqual(events[2][2], {"DENIED": True})
        # No address is handed over: the denied pods are not reached by anyone.
        self.assertFalse(any("TARGETS" in e[2] for e in events if e[0] == "start"))

    def test_every_client_is_awaited_and_the_recovering_control_runs_last(self):
        events = self.run_all()
        waited = [e[1] for e in events if e[0] == "wait"]
        self.assertEqual(waited[:3], ["denied", "ingress-denied", "control"])
        self.assertEqual(sorted(waited), sorted(
            ["control", "denied", "ingress-denied", "control-after", "ingress-control-after"]))
        started = [e for e in events if e[0] == "start"]
        self.assertEqual(started[-1][1:], ("ingress-control-after", {"RECOVER": probe.RECOVER_SECONDS}))
        # No client of stage 2 starts before every client of stage 1 is done.
        self.assertLess(events.index(("wait", "control", None)),
                        [i for i, e in enumerate(events) if e[:2] == ("start", "control-after")][0])

    def test_the_denied_pods_wait_is_bounded_by_the_controls_run(self):
        bounds = [e[2] for e in self.run_all() if e[0] == "wait" and e[1] in probe.DENIED_ROLES]
        self.assertTrue(all(0 < bound <= probe.CONTROL_RUN_SECONDS for bound in bounds), bounds)


def passed(role, started="2026-01-01T00:00:10Z", finished="2026-01-01T00:01:00Z", code=0, message="ok"):
    return {"status": {"phase": "Succeeded", "containerStatuses": [{"state": {"terminated": {
        "exitCode": code, "startedAt": started, "finishedAt": finished, "message": message}}}]}}


class Decision(unittest.TestCase):
    """The runner passes only on the pods' status: exit codes and the order of the times."""

    def check(self, control, **denied):
        probe.check_run(control, {role: pod for role, pod in denied.items()})

    def test_every_pod_clean_and_the_control_last_passes(self):
        self.check(passed("control", "2026-01-01T00:00:00Z", "2026-01-01T00:02:00Z"),
                   denied=passed("denied"), ingress=passed("ingress-denied"))

    def test_a_control_that_ended_before_a_denied_pod_fails_naming_it(self):
        with self.assertRaisesRegex(RuntimeError, "control finished before the denied pod"):
            self.check(passed("control", "2026-01-01T00:00:00Z", "2026-01-01T00:00:30Z"), denied=passed("denied"))

    def test_a_control_that_started_after_a_denied_pod_fails(self):
        with self.assertRaisesRegex(RuntimeError, "control started after the denied pod"):
            self.check(passed("control", "2026-01-01T00:00:20Z", "2026-01-01T00:02:00Z"), denied=passed("denied"))

    def test_a_pod_with_a_nonzero_exit_fails_with_its_message(self):
        with self.assertRaisesRegex(RuntimeError, "denied pod ended with exit code 1: FAILED: boom"):
            self.check(passed("control", "2026-01-01T00:00:00Z", "2026-01-01T00:02:00Z"),
                       denied=passed("denied", code=1, message="FAILED: boom"))

    def test_a_pod_that_has_not_finished_fails(self):
        running = {"status": {"phase": "Running", "containerStatuses": [{"state": {"running": {}}}]}}
        with self.assertRaisesRegex(RuntimeError, "denied pod had not finished"):
            self.check(passed("control", "2026-01-01T00:00:00Z", "2026-01-01T00:02:00Z"), denied=running)

    def test_a_denied_pod_that_never_finishes_is_named_when_the_wait_runs_out(self):
        clock = [0.0]
        with self.assertRaisesRegex(RuntimeError, "denied pod did not finish before the control did"):
            probe.wait_for_pod(
                lambda *a, **k: {"spec": {}, "status": {"phase": "Running"}}, "/pods", "p", "denied",
                lambda pod: False, 3, lambda r: None, "HINT",
                now=lambda: clock.__setitem__(0, clock[0] + 1) or clock[0], sleep=lambda s: None,
                out=lambda *a, **k: None,
                unfinished="the denied pod did not finish before the control did: no refusal was shown to hold")


if __name__ == "__main__":
    unittest.main()
