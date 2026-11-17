"""The CNI probe's wait for a client pod: a pod the runtime could not start is not a verdict."""

from loader import probe
import unittest
import urllib.error

START_MESSAGE = ("failed to create containerd task: failed to create shim task: "
                 "guest did not see hot-plugged drive image_0 within 10s")


def pod(phase, terminated=None, waiting=None, node="node-1", ip="10.0.0.5"):
    state = {}
    if terminated is not None:
        state["terminated"] = terminated
    if waiting is not None:
        state["waiting"] = waiting
    return {"spec": {"nodeName": node},
            "status": {"phase": phase, "podIP": ip, "containerStatuses": [{"name": "probe", "state": state}]}}


# The kubelet's report for a container that was never started: no start time.
NEVER_STARTED = {"exitCode": 128, "reason": "StartError", "message": START_MESSAGE,
                 "startedAt": "1970-01-01T00:00:00Z"}
SUCCEEDED = pod("Succeeded", {"exitCode": 0, "startedAt": "2026-01-01T00:00:00Z", "message": ""})
# A probe that ran, reached the listener for its whole window, and said so itself.
RAN_AND_FAILED = pod("Failed", {"exitCode": 1, "startedAt": "2026-01-01T00:00:00Z",
                                "message": "RuntimeError: NetworkPolicy did not block traffic"})


class Cluster:
    """A fake API: each GET of the pod returns the next scripted state (the last one repeats)."""

    def __init__(self, *states):
        self.states = list(states)
        self.gets = 0
        self.recreated = []
        self.clock = 0.0
        self.output = []

    def request(self, method, path, body=None):
        self.gets += 1
        return self.states.pop(0) if len(self.states) > 1 else self.states[0]

    def now(self):
        return self.clock

    def sleep(self, seconds):
        self.clock += seconds

    def say(self, text, flush=False):
        self.output.append(text)

    def wait(self, role="client", pending_bound_deadline=400, created_at=None):
        return probe.wait_for_pod(
            self.request, "/pods", "probe", role,
            lambda p: p["status"]["phase"] == "Succeeded", pending_bound_deadline,
            lambda r: self.recreated.append(r), "FIX HINT",
            now=self.now, sleep=self.sleep, out=self.say, created_at=created_at)


class RuntimeStartFailure(unittest.TestCase):
    def test_a_pod_that_failed_to_start_is_recreated_once_and_then_passes(self):
        cluster = Cluster(pod("Failed", NEVER_STARTED), SUCCEEDED)
        cluster.wait()
        self.assertEqual(cluster.recreated, ["client"])
        self.assertTrue(any(line.startswith("runtime failed to start client pod: " + START_MESSAGE)
                            and line.endswith("; recreated") for line in cluster.output), cluster.output)

    def test_two_start_failures_fail_naming_the_runtime_not_the_network_policy(self):
        cluster = Cluster(pod("Failed", NEVER_STARTED))
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait()
        self.assertEqual(cluster.recreated, ["client"])
        self.assertIn("runtime failed to start client pod: " + START_MESSAGE, str(raised.exception))
        self.assertNotIn("enforcement is unproven", str(raised.exception))

    def test_a_pod_pending_on_a_runtime_error_is_only_slow_within_the_bound(self):
        stuck = pod("Pending", waiting={"reason": "CreateContainerError", "message": START_MESSAGE})
        cluster = Cluster(*([stuck] * 10 + [SUCCEEDED]))
        cluster.wait()
        self.assertEqual(cluster.recreated, [])

    def test_a_pod_pending_on_a_runtime_error_past_the_bound_is_recreated(self):
        stuck = pod("Pending", waiting={"reason": "CreateContainerError", "message": START_MESSAGE})
        cluster = Cluster(*([stuck] * (probe.START_BOUND_SECONDS + 10) + [SUCCEEDED]))
        cluster.wait()
        self.assertEqual(cluster.recreated, ["client"])


class PodCreatedBeforeItsWait(unittest.TestCase):
    def test_a_pod_already_stuck_past_the_bound_when_its_wait_begins_is_recreated_at_once(self):
        stuck = pod("Pending", waiting={"reason": "CreateContainerError", "message": START_MESSAGE})
        cluster = Cluster(stuck, SUCCEEDED)
        cluster.clock = 100.0
        # Created 60 s ago, while an earlier pod was being waited for.
        cluster.wait(created_at=100.0 - (probe.START_BOUND_SECONDS + 15))
        self.assertEqual(cluster.recreated, ["client"])
        self.assertEqual(cluster.clock, 101.0)

    def test_a_pod_pending_past_the_cause_bound_when_its_wait_begins_fails_at_once(self):
        unschedulable = {"spec": {}, "status": {"phase": "Pending", "conditions": [
            {"type": "PodScheduled", "status": "False", "reason": "Unschedulable",
             "message": "no nodes"}]}}
        cluster = Cluster(unschedulable)
        cluster.clock = 500.0
        with self.assertRaises(RuntimeError):
            cluster.wait(pending_bound_deadline=900,
                         created_at=500.0 - (probe.PENDING_CAUSE_BOUND_SECONDS + 5))
        self.assertEqual(cluster.clock, 500.0)


class ProbeThatRan(unittest.TestCase):
    def test_a_probe_that_ran_and_failed_keeps_the_unproven_verdict_without_a_retry(self):
        cluster = Cluster(RAN_AND_FAILED)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait()
        self.assertEqual(cluster.recreated, [])
        self.assertIn("client probe failed: NetworkPolicy enforcement is unproven", str(raised.exception))
        self.assertIn("NetworkPolicy did not block traffic", str(raised.exception))

    def test_a_pod_that_ran_and_was_lost_with_its_node_fails_and_is_not_recreated(self):
        # ContainerStatusUnknown: exit 137, no start time. It may have run and connected.
        lost = pod("Failed", {"exitCode": 137, "reason": "ContainerStatusUnknown",
                              "message": "The container could not be located when the pod was deleted."})
        cluster = Cluster(lost)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait()
        self.assertEqual(cluster.recreated, [])
        self.assertIn("enforcement is unproven", str(raised.exception))

    def test_a_pod_that_ran_and_failed_with_no_start_time_is_not_recreated(self):
        cluster = Cluster(pod("Failed", {"exitCode": 1, "message": "RuntimeError: boom"}))
        with self.assertRaises(RuntimeError):
            cluster.wait()
        self.assertEqual(cluster.recreated, [])

    def test_a_pod_that_succeeds_is_returned_unchanged(self):
        cluster = Cluster(pod("Running"), SUCCEEDED)
        self.assertEqual(cluster.wait()["status"]["phase"], "Succeeded")
        self.assertEqual(cluster.recreated, [])


UNSCHEDULABLE = {"spec": {}, "status": {"phase": "Pending", "conditions": [
    {"type": "PodScheduled", "status": "False", "reason": "Unschedulable",
     "message": "0/3 nodes are available: 3 Insufficient memory."}]}}
IMAGE_PULL = pod("Pending", waiting={"reason": "ImagePullBackOff",
                                     "message": "Back-off pulling image \"registry.example/probe\""})


class PodThatNeverRuns(unittest.TestCase):
    def test_an_unschedulable_pod_past_the_bound_names_why_and_is_not_recreated(self):
        cluster = Cluster(UNSCHEDULABLE)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait()
        text = str(raised.exception)
        self.assertIn("Unschedulable", text)
        self.assertIn("0/3 nodes are available: 3 Insufficient memory.", text)
        self.assertNotIn("enforcement is unproven", text)
        self.assertNotIn("NetworkPolicy enforcement", text.replace("NetworkPolicy was not tested", ""))
        self.assertEqual(cluster.recreated, [])
        self.assertLessEqual(cluster.clock, probe.PENDING_CAUSE_BOUND_SECONDS + 2)

    def test_an_image_that_does_not_pull_past_the_bound_names_why_and_is_not_recreated(self):
        cluster = Cluster(IMAGE_PULL)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait()
        self.assertIn("ImagePullBackOff", str(raised.exception))
        self.assertIn("Back-off pulling image", str(raised.exception))
        self.assertNotIn("enforcement is unproven", str(raised.exception))
        self.assertEqual(cluster.recreated, [])

    def test_a_pod_pending_within_the_bound_is_only_slow(self):
        cluster = Cluster(*([UNSCHEDULABLE] * (probe.PENDING_CAUSE_BOUND_SECONDS - 10) + [SUCCEEDED]))
        cluster.wait()
        self.assertEqual(cluster.recreated, [])

    def test_the_deadline_of_a_pending_pod_names_its_cause_too(self):
        cluster = Cluster(UNSCHEDULABLE)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait(pending_bound_deadline=30)
        self.assertIn("Insufficient memory", str(raised.exception))
        self.assertNotIn("enforcement is unproven", str(raised.exception))


def http_error(code):
    return urllib.error.HTTPError("https://api", code, "status", {}, None)


class FakeApi:
    """A pod deleted with no grace period stays visible (Terminating) for `lingers` reads."""

    def __init__(self, lingers, present=True):
        self.lingers = lingers
        self.present = present
        self.calls = []
        self.clock = 0.0

    def request(self, method, path, body=None):
        self.calls.append(method)
        if method == "DELETE":
            if not self.present:
                raise http_error(404)
            self.present = "terminating"
            return {}
        if method == "GET":
            if self.present == "terminating":
                if self.lingers == 0:
                    self.present = False
                else:
                    self.lingers -= 1
            if self.present:
                return {}
            raise http_error(404)
        if method == "POST":
            if self.present:
                raise http_error(409)
            self.present = True
            return {"metadata": {"name": body["metadata"]["name"]}}

    def replace(self, deadline=100):
        resource = {"metadata": {"name": "probe-client"}}
        return probe.replace_object(
            self.request, "/pods", resource, deadline,
            now=lambda: self.clock, sleep=lambda seconds: setattr(self, "clock", self.clock + seconds))


class Recreate(unittest.TestCase):
    def test_the_new_pod_is_created_only_after_the_old_one_is_gone(self):
        api = FakeApi(lingers=3)
        self.assertEqual(api.replace()["metadata"]["name"], "probe-client")
        self.assertEqual(api.calls[0], "DELETE")
        self.assertEqual(api.calls[-1], "POST")
        self.assertEqual(api.calls.count("POST"), 1)

    def test_a_pod_that_is_already_gone_is_created(self):
        api = FakeApi(lingers=0, present=False)
        api.replace()
        self.assertEqual(api.calls[-1], "POST")

    def test_a_pod_that_stays_terminating_past_the_deadline_fails_clearly_not_with_a_409(self):
        api = FakeApi(lingers=10**6)
        with self.assertRaisesRegex(RuntimeError, "still being deleted"):
            api.replace(deadline=5)
        self.assertNotIn("POST", api.calls)


if __name__ == "__main__":
    unittest.main()
