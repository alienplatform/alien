"""The whole probe run (`run_probe`) against a fake cluster with a clock.

The fake models what the clusters do: pods start after a delay, a pool that scales from zero
provisions a node first, the route to the listener comes up at some moment, and NetworkPolicy is
enforced for egress and/or ingress or not at all.
"""

import io
import json
import unittest
import urllib.error

from loader import probe

START = 1000.0
MARKER_FAILED = "FAILED: NetworkPolicy did not block traffic"


def http_error(code, text=""):
    return urllib.error.HTTPError("https://api", code, "status", {}, io.BytesIO(text.encode()))


class FakeCluster:
    def __init__(self, egress=True, ingress=True, route_up_at=0, node_up_at=0, start_delay=35,
                 role_pending=None, policy_lands_late=False, first_start_fails=(), fail_with=None,
                 unpinned_nodes=None, ignore_pin=False):
        self.egress, self.ingress = egress, ingress
        self.route_up_at, self.node_up_at = START + route_up_at, (None if node_up_at is None else START + node_up_at)
        self.start_delay = start_delay
        self.role_pending = role_pending or {}
        self.policy_lands_late = policy_lands_late
        self.first_start_fails = set(first_start_fails)
        self.fail_with = fail_with or {}
        # Where the scheduler puts a pod with no pin (default: the listener's node, kata-1). A pod
        # on any other node cannot route to the listener; with ignore_pin the pin is not honored.
        self.unpinned_nodes = unpinned_nodes or {}
        self.ignore_pin = ignore_pin
        self.clock = START
        self.pods = {}       # name -> {"spec", "created", "attempt"}
        self.attempts = {}
        self.log = []        # (time, role, "created"|"finished")
        self.output = []

    # the clock
    def now(self):
        return self.clock

    def sleep(self, seconds):
        self.clock += seconds

    def out(self, text, flush=False):
        self.output.append(text)

    # the API
    def request(self, method, path, body=None):
        if "/jobs/" in path:
            return {"metadata": {"uid": "uid-1"}}
        if method == "POST" and path.endswith("?dryRun=All"):
            if "/pods" in path:
                raise http_error(422, "ValidatingAdmissionPolicy 'pol' denied: probe pods carry no service account token")
            raise http_error(422, "ValidatingAdmissionPolicy 'pol-policies' denied: probe NetworkPolicies use the probe's own names")
        if "/pods" not in path:
            return {"metadata": {"name": (body or {}).get("metadata", {}).get("name", "x")}}
        if method == "POST":
            name = body["metadata"]["name"]
            if name in self.pods:
                raise http_error(409)
            role = body["metadata"]["labels"]["alien.dev/cni-role"]
            self.attempts[role] = self.attempts.get(role, 0) + 1
            self.pods[name] = {"spec": body, "created": self.clock, "attempt": self.attempts[role], "role": role}
            self.log.append((self.clock, role, "created"))
            return {"metadata": {"name": name}}
        name = path.rsplit("/", 1)[1]
        if method == "DELETE":
            if self.pods.pop(name, None) is None:
                raise http_error(404)
            return {}
        if name not in self.pods:
            raise http_error(404)
        return self.status(self.pods[name])

    # what a pod does
    def scheduled_at(self, pod):
        if self.node_up_at is None:
            return None
        return max(pod["created"] + self.role_pending.get(pod["role"], 0), self.node_up_at)

    def finish(self, pod, started):
        """(finish time, exit code, message) of a client that started at `started`."""
        role = pod["role"]
        on_listener_node = self.node_of(pod) == "kata-1"
        route_up = lambda t: t >= self.route_up_at and on_listener_node
        if role in ("control", "control-after"):
            return started + 3, 0 if route_up(started) else 1, "reached" if route_up(started) else "FAILED: URLError"
        if role == "ingress-control-after":
            resumed = max(started + 3, self.route_up_at + 1)
            if resumed - started > probe.RECOVER_SECONDS:
                return started + probe.RECOVER_SECONDS, 1, "FAILED: traffic did not resume"
            return resumed, 0, "reached"
        enforced = self.egress if role == "denied" else self.ingress
        if enforced and self.policy_lands_late:
            # The policy lands a few seconds after the pod starts; it has landed in time only
            # when an earlier client pod already ran on the node and warmed the CNI.
            enforced = any(self.finish_time(other) <= pod["created"]
                           for other in self.pods.values()
                           if other["role"] != "listener" and other["created"] < pod["created"])
        refused = enforced or not route_up(started)
        if refused:
            return started + 3, 0, "refused on all 3 attempts"
        return started + 1, 1, MARKER_FAILED

    def node_of(self, pod):
        pin = pod["spec"]["spec"].get("nodeSelector", {}).get("kubernetes.io/hostname")
        if pin and not self.ignore_pin:
            return pin
        return self.unpinned_nodes.get(pod["role"], "kata-1")

    def finish_time(self, pod):
        scheduled = self.scheduled_at(pod)
        if scheduled is None:
            return float("inf")
        return self.finish(pod, scheduled + self.start_delay)[0]

    def status(self, pod):
        role, t = pod["role"], self.clock
        scheduled = self.scheduled_at(pod)
        if scheduled is None or t < scheduled:
            return {"spec": {}, "status": {"phase": "Pending", "conditions": [
                {"type": "PodScheduled", "status": "False", "reason": "Unschedulable",
                 "message": "0/3 nodes are available: 3 Insufficient memory."}]}}
        started = scheduled + self.start_delay
        spec = {"nodeName": self.node_of(pod)}
        if t < started:
            return {"spec": spec, "status": {"phase": "Pending", "containerStatuses": [
                {"name": "probe", "state": {"waiting": {"reason": "ContainerCreating"}}}]}}
        if role in self.fail_with:
            return {"spec": spec, "status": self.fail_with[role]}
        if role in self.first_start_fails and pod["attempt"] == 1:
            return {"spec": spec, "status": {"phase": "Failed", "containerStatuses": [{"name": "probe", "state": {
                "terminated": {"exitCode": 128, "reason": "StartError", "message": "shim did not start",
                               "startedAt": "1970-01-01T00:00:00Z"}}}]}}
        if role == "listener":
            return {"spec": spec, "status": {"phase": "Running", "podIP": "10.0.0.9",
                                            "conditions": [{"type": "Ready", "status": "True"}]}}
        done, code, message = self.finish(pod, started)
        if t < done:
            return {"spec": spec, "status": {"phase": "Running", "podIP": "10.0.0.5"}}
        if (done, role, "finished") not in self.log:
            self.log.append((done, role, "finished"))
        return {"spec": spec, "status": {"phase": "Succeeded" if code == 0 else "Failed", "podIP": "10.0.0.5",
                                        "containerStatuses": [{"name": "probe", "state": {"terminated": {
                                            "exitCode": code, "message": message,
                                            "startedAt": "2026-01-01T00:00:00Z"}}}]}}

    def run(self):
        env = {"PROBE_NAMESPACE": "ns", "PROBE_JOB": "job", "PROBE_IMAGE": "img", "PROBE_RUNTIME_CLASS": "kata-vm",
               "PROBE_ADMISSION_POLICY": "pol", "PROBE_NODE_SELECTOR": json.dumps({"pool": "kata"}),
               "PROBE_TOLERATIONS": "[]"}
        probe.run_probe(self.request, env, self.now, self.sleep, self.out)
        return self

    def created_at(self, role):
        return min(t for t, r, what in self.log if r == role and what == "created")

    def finished_at(self, role):
        return min(t for t, r, what in self.log if r == role and what == "finished")

    def elapsed(self):
        return self.clock - START


class Passing(unittest.TestCase):
    def test_a_cluster_that_enforces_both_directions_passes_inside_the_budget(self):
        cluster = FakeCluster().run()
        self.assertIn("NetworkPolicy enforced", cluster.output[-1])
        self.assertLess(cluster.elapsed(), probe.SCRIPT_BUDGET_SECONDS)

    def test_the_control_succeeds_alone_before_any_denied_pod_starts(self):
        cluster = FakeCluster().run()
        control_done = cluster.finished_at("control")
        for role in ("denied", "ingress-denied"):
            self.assertGreaterEqual(cluster.created_at(role), control_done, role)
        self.assertEqual(cluster.created_at("denied"), cluster.created_at("ingress-denied"))
        for role in ("control-after", "ingress-control-after"):
            self.assertGreaterEqual(cluster.created_at(role), cluster.finished_at("denied"), role)
            self.assertGreaterEqual(cluster.created_at(role), cluster.finished_at("ingress-denied"), role)
        self.assertEqual(cluster.created_at("control-after"), cluster.created_at("ingress-control-after"))

    def test_every_pod_runs_on_the_sandbox_nodes_in_the_sandbox_runtime_class(self):
        cluster = FakeCluster()
        created = []
        original = cluster.request
        cluster.request = lambda m, p, b=None: (created.append(b) if m == "POST" and b and "spec" in b
                                                and "labels" in b["metadata"] and "?" not in p else None) or original(m, p, b)
        cluster.run()
        roles = {pod["metadata"]["labels"]["alien.dev/cni-role"] for pod in created}
        self.assertIn("listener", roles)
        for pod in created:
            self.assertEqual(pod["spec"]["runtimeClassName"], "kata-vm")
            self.assertEqual(pod["spec"]["nodeSelector"].get("pool"), "kata")

    def test_a_pool_that_scales_from_zero_passes(self):
        # The listener is the first pod on the sandbox nodes and waits for the node.
        cluster = FakeCluster(node_up_at=180).run()
        self.assertIn("NetworkPolicy enforced", cluster.output[-1])

    def test_a_client_pending_150_seconds_for_a_node_then_running_passes(self):
        cluster = FakeCluster(role_pending={"control": 150}).run()
        self.assertIn("NetworkPolicy enforced", cluster.output[-1])

    def test_a_control_the_runtime_never_started_is_recreated_before_any_denied_pod(self):
        cluster = FakeCluster(first_start_fails={"control"}).run()
        self.assertTrue(any("failed to start the control pod" in line for line in cluster.output), cluster.output)
        self.assertGreaterEqual(cluster.created_at("denied"), cluster.finished_at("control"))

    def test_a_policy_that_lands_seconds_after_a_pod_starts_passes_only_with_the_control_first(self):
        # On EKS the policy reaches a pod 1-3 s after it starts; the control that ran first
        # gave the node's policy agent that time.
        FakeCluster(policy_lands_late=True).run()
        together = [[(role, denied, recover) for stage in probe.STAGES for role, denied, recover in stage]]
        original, probe.STAGES = probe.STAGES, together
        try:
            with self.assertRaisesRegex(RuntimeError, "did not block"):
                FakeCluster(policy_lands_late=True).run()
        finally:
            probe.STAGES = original


class SameNode(unittest.TestCase):
    def test_every_pod_after_the_first_control_is_pinned_to_its_node(self):
        cluster = FakeCluster(unpinned_nodes={"denied": "kata-2", "ingress-denied": "kata-2"})
        created = []
        original = cluster.request
        cluster.request = lambda m, p, b=None: (created.append(b) if m == "POST" and b and "spec" in b
                                                and "labels" in b["metadata"] and "?" not in p else None) or original(m, p, b)
        cluster.run()
        self.assertIn("NetworkPolicy enforced", cluster.output[-1])
        pins = {pod["metadata"]["labels"]["alien.dev/cni-role"]:
                pod["spec"]["nodeSelector"].get("kubernetes.io/hostname") for pod in created}
        self.assertIsNone(pins["control"])
        for role in ("denied", "ingress-denied", "control-after", "ingress-control-after"):
            self.assertEqual(pins[role], "kata-1", role)

    def test_a_denied_pod_on_another_node_than_the_control_fails(self):
        # The pin is not honored and the denied pod lands on a node that cannot route to the
        # listener: every attempt would time out and the old check passed.
        cluster = FakeCluster(unpinned_nodes={"denied": "kata-2"}, ignore_pin=True)
        with self.assertRaisesRegex(RuntimeError, "denied pod ran on node kata-2, not on kata-1") as raised:
            cluster.run()
        self.assertNotIn(probe.FIX_HINT, str(raised.exception))

    def test_a_control_with_no_node_fails(self):
        cluster = FakeCluster()
        original = cluster.status
        cluster.status = lambda pod: {**original(pod), "spec": {}} if pod["role"] == "control" else original(pod)
        with self.assertRaisesRegex(RuntimeError, "reports no node"):
            cluster.run()


class Failing(unittest.TestCase):
    def fails(self, cluster, pattern):
        with self.assertRaises(RuntimeError) as raised:
            cluster.run()
        self.assertRegex(str(raised.exception), pattern)
        return str(raised.exception)

    def test_no_enforcement_fails_with_the_unenforced_verdict_and_the_hint(self):
        text = self.fails(FakeCluster(egress=False, ingress=False), "did not block traffic")
        self.assertIn(probe.FIX_HINT, text)

    def test_egress_only_enforcement_fails_at_the_ingress_pod(self):
        text = self.fails(FakeCluster(egress=True, ingress=False), "ingress-denied pod reached")
        self.assertIn(probe.FIX_HINT, text)

    def test_ingress_only_enforcement_fails_at_the_egress_pod(self):
        text = self.fails(FakeCluster(egress=False, ingress=True), "denied pod reached")
        self.assertIn(probe.FIX_HINT, text)

    def test_a_late_route_on_a_cluster_that_enforces_nothing_fails(self):
        # The route to the listener is not up when the control runs: the old order judged the
        # denied pod "refused" and the later control passed.
        cluster = FakeCluster(egress=False, ingress=False, route_up_at=120)
        self.fails(cluster, "control pod failed")
        self.assertFalse(any(role == "denied" for _, role, _ in cluster.log), "no denied pod may start")

    def test_a_route_that_comes_up_between_the_control_and_the_denied_pods_still_fails(self):
        cluster = FakeCluster(egress=False, ingress=False, route_up_at=0)
        self.fails(cluster, "did not block traffic")

    def test_a_full_pool_fails_naming_unschedulable_with_no_hint(self):
        text = self.fails(FakeCluster(node_up_at=None), "never ran.*Unschedulable|did not finish.*Unschedulable")
        self.assertNotIn("NetworkPolicy to be enforced", text)
        self.assertNotIn(probe.FIX_HINT, text)

    def test_a_failed_control_reports_its_own_failure_and_no_hint(self):
        text = self.fails(FakeCluster(route_up_at=500), "control pod failed.*FAILED: URLError")
        self.assertNotIn(probe.FIX_HINT, text)

    def test_an_oom_killed_pod_names_oom_killed_with_no_hint(self):
        oom = {"phase": "Failed", "containerStatuses": [{"name": "probe", "state": {"terminated": {
            "exitCode": 137, "reason": "OOMKilled", "message": "", "startedAt": "2026-01-01T00:00:00Z"}}}]}
        text = self.fails(FakeCluster(fail_with={"denied": oom}), "OOMKilled")
        self.assertIn("137", text)
        self.assertNotIn(probe.FIX_HINT, text)

    def test_an_evicted_pod_names_the_eviction_with_no_hint(self):
        evicted = {"phase": "Failed", "reason": "Evicted", "message": "The node was low on resource: memory."}
        text = self.fails(FakeCluster(fail_with={"control": evicted}), "Evicted.*low on resource")
        self.assertNotIn(probe.FIX_HINT, text)


if __name__ == "__main__":
    unittest.main()
