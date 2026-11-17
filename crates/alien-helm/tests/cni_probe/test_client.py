"""The client pods' verdicts: refusals count only while the listener and the denied pod are up."""

import http.server
import socket
import threading
import time
import unittest

from loader import probe

MARKER = "marker-for-the-test"
FAST = dict(WINDOW=4, HOLD=2, LINGER=1, SERVED_WAIT=4, SERVE_WAIT=3, CONTROL_RUN=20)


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class Listener:
    """Answers HTTP requests with the marker until stopped (`url` of a stopped one is refused)."""

    def __init__(self, serve=True, port=None):
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                self.end_headers()
                self.wfile.write(MARKER.encode())

            def log_message(self, *args):
                pass

        self.port = port or free_port()
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", self.port), Handler) if serve else None
        if self.server:
            threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def stop(self):
        if self.server:
            self.server.shutdown()
            self.server.server_close()
            self.server = None

    def url(self):
        return f"http://127.0.0.1:{self.port}/"


class Pod:
    """A client pod's code run in a thread, with its outcome."""

    def __init__(self, listener, **settings):
        self.error = None
        self.log = None
        code = probe.client_code(listener.url(), MARKER)
        scope = {"LOG": "/dev/null", "BIND": "127.0.0.1", **FAST, **settings}
        self.thread = threading.Thread(target=self._run, args=(code, scope), daemon=True)
        self.thread.start()

    def _run(self, code, scope):
        try:
            exec(compile(code, "client", "exec"), scope)
        except Exception as error:  # the pod's exit status
            self.error = error

    def result(self, timeout=30):
        self.thread.join(timeout)
        assert not self.thread.is_alive(), "the client did not finish"
        return self.error


def denied(listener, **settings):
    """A denied pod: serves on a free port of its own; returns the pod and its address."""
    port = free_port()
    return Pod(listener, DENIED=True, PORT=port, **settings), f"http://127.0.0.1:{port}/"


class DeniedPodsAndControl(unittest.TestCase):
    def test_refusals_with_a_listener_that_stays_up_pass(self):
        up = Listener()
        refused = Listener(serve=False)  # what enforcement does to the denied pods
        first, first_address = denied(refused)
        second, second_address = denied(refused)
        control = Pod(up, TARGETS=[first_address, second_address])
        try:
            self.assertIsNone(control.result())
            self.assertIsNone(first.result())
            self.assertIsNone(second.result())
        finally:
            up.stop()

    def test_a_listener_outage_partway_with_no_enforcement_fails_the_control(self):
        # No NetworkPolicy at all: the denied pod reaches the listener, then the listener goes
        # away, and the denied pod sees only refusals. Its own verdict would be a pass.
        listener = Listener()
        pod, address = denied(listener, WINDOW=8, HOLD=3)
        control = Pod(listener, TARGETS=[address])
        time.sleep(2)
        listener.stop()
        self.assertRegex(str(control.result()), "listener unreachable from the control")
        self.assertIsNone(pod.result())

    def test_an_unreachable_denied_pod_fails_instead_of_counting_refusals(self):
        refused = Listener(serve=False)
        pod, _ = denied(refused, SERVED_WAIT=2)  # no control ever reaches it
        self.assertRegex(str(pod.result()), "denied pod network is not up")

    def test_a_control_that_cannot_reach_a_denied_pod_fails(self):
        up = Listener()
        try:
            control = Pod(up, TARGETS=[f"http://127.0.0.1:{free_port()}/"], SERVE_WAIT=2)
            self.assertRegex(str(control.result()), "denied pod unreachable from the control")
        finally:
            up.stop()

    def test_a_denied_pod_that_stops_answering_before_it_is_done_fails_the_control(self):
        up = Listener()
        fake = Listener()  # answers the marker, never says `done`: a denied pod that is counting
        control = Pod(up, TARGETS=[fake.url()], CONTROL_RUN=20)
        time.sleep(1.5)
        fake.stop()
        try:
            self.assertRegex(str(control.result()), "denied pod unreachable")
        finally:
            up.stop()

    def test_traffic_that_never_stops_fails_the_denied_pod(self):
        up = Listener()
        pod, address = denied(up)
        control = Pod(up, TARGETS=[address])
        try:
            self.assertRegex(str(pod.result()), "NetworkPolicy did not block traffic")
            control.result()
        finally:
            up.stop()

    def test_traffic_that_resumes_after_a_refusal_fails_the_denied_pod(self):
        port = free_port()
        flapping = Listener(serve=False, port=port)
        up = Listener()
        pod, address = denied(flapping, HOLD=4)
        control = Pod(up, TARGETS=[address])
        time.sleep(1.5)
        flapping = Listener(port=port)
        try:
            self.assertRegex(str(pod.result()), "reachable again")
            control.result()
        finally:
            flapping.stop()
            up.stop()


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

        def wait(role):
            events.append(("wait", role))

        def running(role):
            events.append(("running", role))
            return {"status": {"podIP": "10.0.0." + str(len(events))}}

        probe.run_stages(start, wait, running)
        return events

    def test_the_denied_pods_start_first_and_the_control_gets_their_addresses(self):
        events = self.run_all()
        kinds = [(e[0], e[1]) for e in events]
        self.assertEqual(kinds[:5], [
            ("start", "denied"), ("start", "ingress-denied"),
            ("running", "denied"), ("running", "ingress-denied"), ("start", "control")])
        control = events[4][2]
        self.assertEqual(len(control["TARGETS"]), 2)
        self.assertTrue(all(address.startswith("10.0.0.") for address in control["TARGETS"]))
        self.assertEqual(events[0][2], {"DENIED": True})

    def test_every_client_is_awaited_and_the_recovering_control_runs_last(self):
        events = self.run_all()
        waited = [e[1] for e in events if e[0] == "wait"]
        self.assertEqual(sorted(waited), sorted(
            ["control", "denied", "ingress-denied", "control-after", "ingress-control-after"]))
        self.assertEqual(waited[:3], ["control", "denied", "ingress-denied"])
        started = [e for e in events if e[0] == "start"]
        self.assertEqual(started[-1][1:], ("ingress-control-after", {"RECOVER": probe.RECOVER_SECONDS}))
        # No client of stage 2 starts before every client of stage 1 is done.
        self.assertLess(events.index(("wait", "ingress-denied")),
                        [i for i, e in enumerate(events) if e[:2] == ("start", "control-after")][0])


if __name__ == "__main__":
    unittest.main()
