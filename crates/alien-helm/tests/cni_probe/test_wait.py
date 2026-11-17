"""The runner's wait for a probe pod: it decides from the pod's status and names what it saw."""

import unittest
import urllib.error

from loader import probe

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
SUCCEEDED = pod("Succeeded", {"exitCode": 0, "startedAt": "2026-01-01T00:00:00Z",
                              "message": "refused on all 3 attempts"})
RAN_AND_FAILED = pod("Failed", {"exitCode": 1, "startedAt": "2026-01-01T00:00:00Z",
                                "message": "FAILED: NetworkPolicy did not block traffic"})
UNSCHEDULABLE = {"spec": {}, "status": {"phase": "Pending", "conditions": [
    {"type": "PodScheduled", "status": "False", "reason": "Unschedulable",
     "message": "0/3 nodes are available: 3 Insufficient memory."}]}}
IMAGE_PULL = pod("Pending", waiting={"reason": "ImagePullBackOff",
                                     "message": "Back-off pulling image \"registry.example/probe\""})


class Cluster:
    """A fake API: each GET of the pod returns the next scripted state (the last one repeats)."""

    def __init__(self, *states):
        self.states = list(states)
        self.recreated = []
        self.clock = 0.0
        self.output = []

    def request(self, method, path, body=None):
        return self.states.pop(0) if len(self.states) > 1 else self.states[0]

    def wait(self, role="client", deadline=400, **extra):
        return probe.wait_for_pod(
            self.request, "/pods", "probe", role,
            lambda p: p["status"]["phase"] == "Succeeded", deadline,
            lambda r: self.recreated.append(r),
            now=lambda: self.clock,
            sleep=lambda s: setattr(self, "clock", self.clock + s),
            out=lambda text, flush=False: self.output.append(text), **extra)


class RuntimeNeverStartedThePod(unittest.TestCase):
    def test_a_pod_that_failed_to_start_is_recreated_once_and_named(self):
        cluster = Cluster(pod("Failed", NEVER_STARTED), SUCCEEDED)
        cluster.wait()
        self.assertEqual(cluster.recreated, ["client"])
        self.assertTrue(any(line.startswith("the runtime failed to start the client pod: " + START_MESSAGE)
                            and line.endswith("; recreated") for line in cluster.output), cluster.output)

    def test_a_second_start_failure_names_the_runtime_not_the_network_policy(self):
        cluster = Cluster(pod("Failed", NEVER_STARTED))
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait()
        self.assertEqual(cluster.recreated, ["client"])
        self.assertIn("the runtime failed to start the client pod twice: " + START_MESSAGE, str(raised.exception))
        self.assertNotIn("enforcement is unproven", str(raised.exception))

    def test_a_pod_waiting_on_a_runtime_error_is_recreated_only_past_the_bound(self):
        stuck = pod("Pending", waiting={"reason": "CreateContainerError", "message": START_MESSAGE})
        within = Cluster(*([stuck] * 10 + [SUCCEEDED]))
        within.wait()
        self.assertEqual(within.recreated, [])
        past = Cluster(*([stuck] * (probe.RUNTIME_START_BOUND_SECONDS + 10) + [SUCCEEDED]))
        past.wait()
        self.assertEqual(past.recreated, ["client"])

    def test_a_pod_created_long_before_its_wait_began_counts_from_its_creation(self):
        stuck = pod("Pending", waiting={"reason": "CreateContainerError", "message": START_MESSAGE})
        cluster = Cluster(stuck, SUCCEEDED)
        cluster.clock = 100.0
        cluster.wait(created_at=100.0 - (probe.RUNTIME_START_BOUND_SECONDS + 15))
        self.assertEqual(cluster.recreated, ["client"])


class PodThatRan(unittest.TestCase):
    def test_a_probe_that_ran_and_failed_keeps_its_verdict_without_a_retry(self):
        cluster = Cluster(RAN_AND_FAILED)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait()
        self.assertEqual(cluster.recreated, [])
        self.assertIn("the client pod reached the listener it must not reach", str(raised.exception))
        self.assertIn("FAILED: NetworkPolicy did not block traffic", str(raised.exception))
        self.assertIn(probe.FIX_HINT, str(raised.exception))

    def test_a_pod_that_failed_for_another_reason_reports_it_and_no_hint(self):
        control = pod("Failed", {"exitCode": 1, "startedAt": "2026-01-01T00:00:00Z",
                                 "message": "FAILED: <urlopen error timed out>"})
        with self.assertRaises(RuntimeError) as raised:
            Cluster(control).wait("control")
        self.assertIn("exit code 1", str(raised.exception))
        self.assertIn("<urlopen error timed out>", str(raised.exception))
        self.assertNotIn(probe.FIX_HINT, str(raised.exception))

    def test_an_oom_killed_pod_with_an_empty_message_names_oom_killed(self):
        oom = pod("Failed", {"exitCode": 137, "reason": "OOMKilled", "message": "",
                             "startedAt": "2026-01-01T00:00:00Z"})
        with self.assertRaises(RuntimeError) as raised:
            Cluster(oom).wait("denied")
        self.assertIn("OOMKilled", str(raised.exception))
        self.assertIn("137", str(raised.exception))
        self.assertNotIn(probe.FIX_HINT, str(raised.exception))

    def test_a_pod_lost_with_its_node_is_not_recreated(self):
        lost = pod("Failed", {"exitCode": 137, "reason": "ContainerStatusUnknown",
                              "message": "The container could not be located when the pod was deleted."})
        cluster = Cluster(lost)
        with self.assertRaises(RuntimeError):
            cluster.wait()
        self.assertEqual(cluster.recreated, [])

    def test_the_verdict_of_a_pod_that_passed_is_printed(self):
        cluster = Cluster(pod("Running"), SUCCEEDED)
        cluster.wait("denied")
        self.assertTrue(any("verdict: refused on all 3 attempts" in line for line in cluster.output), cluster.output)


class PodThatNeverRuns(unittest.TestCase):
    def test_an_unschedulable_pod_names_the_pod_and_the_scheduler_reason(self):
        cluster = Cluster(UNSCHEDULABLE)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait("control")
        text = str(raised.exception)
        self.assertIn("the control pod never ran", text)
        self.assertIn("Unschedulable", text)
        self.assertIn("0/3 nodes are available: 3 Insufficient memory.", text)
        self.assertEqual(cluster.recreated, [])
        self.assertLessEqual(cluster.clock, probe.PENDING_SECONDS + 2)

    def test_an_image_that_does_not_pull_names_the_kubelet_reason(self):
        cluster = Cluster(IMAGE_PULL)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait()
        self.assertIn("ImagePullBackOff", str(raised.exception))
        self.assertIn("Back-off pulling image", str(raised.exception))

    def test_the_deadline_names_the_pod_and_the_reason_it_was_stuck_on(self):
        cluster = Cluster(UNSCHEDULABLE)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait("listener", deadline=30, pending_bound=10**6)
        self.assertIn("the listener pod", str(raised.exception))
        self.assertIn("Insufficient memory", str(raised.exception))

    def test_no_failure_names_one_cloud(self):
        for state in (UNSCHEDULABLE, IMAGE_PULL, pod("Failed", NEVER_STARTED)):
            cluster = Cluster(state)
            with self.assertRaises(RuntimeError) as raised:
                cluster.wait()
            self.assertNotRegex(str(raised.exception), "EKS|GKE|AKS|Auto Mode")

    def test_the_hint_for_an_unenforced_policy_lists_each_provider_by_name(self):
        for cloud in ("EKS Auto Mode", "GKE", "AKS"):
            self.assertIn(cloud, probe.FIX_HINT)
        self.assertNotIn("kubectl apply", probe.FIX_HINT)


def http_error(code):
    return urllib.error.HTTPError("https://api", code, "status", {}, None)


class FakeApi:
    """A pod deleted with no grace period stays visible (Terminating) for `lingers` reads."""

    def __init__(self, lingers, present=True):
        self.lingers, self.present, self.calls, self.clock = lingers, present, [], 0.0

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
        if self.present:
            raise http_error(409)
        self.present = True
        return {"metadata": {"name": body["metadata"]["name"]}}

    def replace(self, deadline=100):
        return probe.replace_object(
            self.request, "/pods", {"metadata": {"name": "probe-client"}}, deadline,
            now=lambda: self.clock, sleep=lambda s: setattr(self, "clock", self.clock + s))


class Recreate(unittest.TestCase):
    def test_the_new_pod_is_created_only_after_the_old_one_is_gone(self):
        api = FakeApi(lingers=3)
        api.replace()
        self.assertEqual((api.calls[0], api.calls[-1], api.calls.count("POST")), ("DELETE", "POST", 1))

    def test_a_pod_that_stays_terminating_past_the_deadline_fails_clearly(self):
        api = FakeApi(lingers=10**6)
        with self.assertRaisesRegex(RuntimeError, "still being deleted"):
            api.replace(deadline=5)
        self.assertNotIn("POST", api.calls)


if __name__ == "__main__":
    unittest.main()
