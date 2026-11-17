"""The CNI probe's wait for a client pod: a pod the runtime could not start is not a verdict."""

import importlib.util
import pathlib
import unittest

SOURCE = pathlib.Path(__file__).parents[2] / "src" / "emitters" / "sandbox_cni_probe.py"
spec = importlib.util.spec_from_file_location("sandbox_cni_probe", SOURCE)
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

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

    def wait(self, role="client", pending_bound_deadline=200):
        return probe.wait_for_pod(
            self.request, "/pods", "probe", role,
            lambda p: p["status"]["phase"] == "Succeeded", pending_bound_deadline,
            lambda r: self.recreated.append(r), "FIX HINT",
            now=self.now, sleep=self.sleep, out=self.say)


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


class ProbeThatRan(unittest.TestCase):
    def test_a_probe_that_ran_and_failed_keeps_the_unproven_verdict_without_a_retry(self):
        cluster = Cluster(RAN_AND_FAILED)
        with self.assertRaises(RuntimeError) as raised:
            cluster.wait()
        self.assertEqual(cluster.recreated, [])
        self.assertIn("client probe failed: NetworkPolicy enforcement is unproven", str(raised.exception))
        self.assertIn("NetworkPolicy did not block traffic", str(raised.exception))

    def test_a_pod_that_succeeds_is_returned_unchanged(self):
        cluster = Cluster(pod("Running"), SUCCEEDED)
        self.assertEqual(cluster.wait()["status"]["phase"], "Succeeded")
        self.assertEqual(cluster.recreated, [])


if __name__ == "__main__":
    unittest.main()
