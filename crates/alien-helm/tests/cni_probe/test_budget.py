"""The probe's time budget: its parts add up and fit the Job and the install's wait."""

import pathlib
import re
import unittest

from loader import probe

RUST = pathlib.Path(__file__).parents[2] / "src" / "emitters" / "sandbox_cni.rs"
INSTALL_WAIT_SECONDS = 900  # `helm install --timeout 15m`, applied by Helm to each hook
NEXT_HOOK_SECONDS = 120  # the root boot check's own pod deadline


class Budget(unittest.TestCase):
    def test_the_script_budget_is_the_sum_of_its_parts(self):
        start = probe.POD_START_SECONDS
        total = (probe.API_READY_SECONDS + probe.ADMISSION_SECONDS
                 + probe.PENDING_CAUSE_BOUND_SECONDS                       # listener (a node may be provisioned)
                 + start + probe.CONTROL_RUN_SECONDS                       # stage 1
                 + start + probe.RECOVER_RUN_SECONDS                       # stage 2
                 + probe.START_BOUND_SECONDS + start                       # one recreate
                 + probe.CLEANUP_SECONDS)
        self.assertEqual(probe.SCRIPT_BUDGET_SECONDS, total)
        self.assertEqual(total, 60 + 30 + 210 + (35 + 147) + (35 + 33) + (45 + 35) + 10)
        self.assertEqual(probe.STAGE_ONE_SECONDS + probe.STAGE_TWO_SECONDS,
                         total - probe.API_READY_SECONDS - probe.ADMISSION_SECONDS
                         - probe.PENDING_CAUSE_BOUND_SECONDS
                         - probe.START_BOUND_SECONDS - start - probe.CLEANUP_SECONDS)

    def test_the_control_outlasts_the_denied_pods_worst_case_with_margin(self):
        # A denied pod starts (35 s) after the control, waits up to 20 s for its first answer on the
        # allowed port (that request: 3 s), counts for a window (45 s) and a hold (10 s), each
        # overrunning by one tick of 7 s: 35 + 23 + 45 + 10 + 14 = 127; margin 20 = 147.
        self.assertEqual(probe.DENIED_WORST_CASE_SECONDS, 20 + 3 + 45 + 10 + 2 * 7)
        self.assertEqual(probe.CONTROL_RUN_SECONDS, 35 + 92 + 20)
        self.assertGreaterEqual(
            probe.CONTROL_RUN_SECONDS,
            probe.POD_START_SECONDS + probe.DENIED_WORST_CASE_SECONDS + probe.CONTROL_RUN_MARGIN_SECONDS)

    def test_a_control_that_cannot_be_scheduled_is_named_by_the_runner(self):
        # A pod after the listener that stays Pending fails naming its cause well before the
        # control's run is over.
        self.assertLess(probe.FOLLOWER_PENDING_CAUSE_BOUND_SECONDS, probe.CONTROL_RUN_SECONDS)
        self.assertLessEqual(probe.FOLLOWER_PENDING_CAUSE_BOUND_SECONDS, probe.PENDING_CAUSE_BOUND_SECONDS)

    def test_a_denied_pod_fits_its_own_pod_deadline(self):
        self.assertGreater(probe.POD_DEADLINE_SECONDS, probe.CONTROL_RUN_SECONDS)

    def test_the_install_instructions_wait_as_long_as_this_test_assumes(self):
        generator = (RUST.parent.parent / "generator.rs").read_text()
        self.assertIn(f"--timeout {INSTALL_WAIT_SECONDS // 60}m", generator)
        # Every place that installs or upgrades the chart waits as long.
        crates = RUST.parents[3]
        wait = f"{INSTALL_WAIT_SECONDS // 60}m"
        self.assertIn(f"--timeout {wait}", (crates / "alien-cli/src/commands/onboard.rs").read_text())
        self.assertIn(f'"--timeout", "{wait}"', (crates / "alien-deploy-cli/src/commands/sync.rs").read_text())
        self.assertIn(f"timeout: {wait}", generator)
        self.assertIn(f'.arg("--timeout")\n        .arg("{wait}")', (crates / "alien-deploy-cli/src/commands/up.rs").read_text())
        self.assertIn(f"--timeout {wait}", (crates.parent / "examples/customer-kubernetes/README.md").read_text())

    def test_the_job_deadline_covers_the_script_the_runner_start_and_a_margin(self):
        self.assertEqual(
            probe.JOB_DEADLINE_SECONDS,
            probe.SCRIPT_BUDGET_SECONDS + probe.RUNNER_START_SECONDS + probe.BUDGET_MARGIN_SECONDS)

    def test_the_job_in_the_chart_uses_that_deadline(self):
        job = re.search(r"kind: Job.*?activeDeadlineSeconds: (\d+|\{[^\n]*)", RUST.read_text(), re.S)
        self.assertIsNotNone(job)
        self.assertNotRegex(job.group(1), r"^\d+$", "the Job's deadline is read from the script, not typed twice")

    def test_the_job_leaves_the_next_hook_room_inside_the_install_wait(self):
        self.assertLessEqual(probe.JOB_DEADLINE_SECONDS + NEXT_HOOK_SECONDS, INSTALL_WAIT_SECONDS)

    def test_a_pending_pod_waits_at_least_as_long_as_the_install_did_before(self):
        self.assertGreaterEqual(probe.PENDING_CAUSE_BOUND_SECONDS, 210)

    def test_a_recovering_control_fits_its_pod_deadline(self):
        self.assertGreaterEqual(
            probe.POD_DEADLINE_SECONDS, probe.POD_START_SECONDS + probe.RECOVER_RUN_SECONDS)

    def test_every_probe_pod_outlives_the_whole_run(self):
        # The listener serves every later control; a pod deadline below the script's would let
        # the kubelet end it while controls still run.
        self.assertGreaterEqual(probe.POD_DEADLINE_SECONDS, probe.SCRIPT_BUDGET_SECONDS)

    def test_a_pod_stuck_on_a_runtime_error_is_recreated_within_the_budgeted_bound(self):
        self.assertLessEqual(probe.START_BOUND_SECONDS, 45)

    def test_the_budget_is_left_when_the_clock_runs_out(self):
        # A deadline already passed fails the wait; it never reports a pass.
        clock = [0.0]
        with self.assertRaises(RuntimeError):
            probe.wait_for_pod(lambda *a, **k: {"spec": {}, "status": {"phase": "Running"}},
                               "/pods", "p", "client", lambda pod: False, 3,
                               lambda r: None, "HINT",
                               now=lambda: clock.__setitem__(0, clock[0] + 1) or clock[0],
                               sleep=lambda s: None, out=lambda *a, **k: None)


if __name__ == "__main__":
    unittest.main()
