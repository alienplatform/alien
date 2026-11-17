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
                 + start + start + probe.CONTROL_RUN_SECONDS + probe.LINGER_SECONDS  # stage 1
                 + start + probe.RECOVER_RUN_SECONDS                       # stage 2
                 + probe.START_BOUND_SECONDS + start                       # one recreate
                 + probe.CLEANUP_SECONDS)
        self.assertEqual(probe.SCRIPT_BUDGET_SECONDS, total)
        self.assertEqual(probe.STAGE_ONE_SECONDS + probe.STAGE_TWO_SECONDS,
                         total - probe.API_READY_SECONDS - probe.ADMISSION_SECONDS
                         - probe.PENDING_CAUSE_BOUND_SECONDS
                         - probe.START_BOUND_SECONDS - start - probe.CLEANUP_SECONDS)

    def test_the_control_outlasts_the_longest_denied_pod(self):
        # A denied pod is served at most SERVE_WAIT after the control starts, then counts for a
        # window and a hold; the control must still be probing the listener until it is done.
        self.assertGreater(
            probe.CONTROL_RUN_SECONDS,
            probe.SERVE_WAIT_SECONDS + probe.ENFORCEMENT_WINDOW_SECONDS + probe.HOLD_SECONDS)

    def test_a_denied_pod_waits_long_enough_for_the_control_to_start_and_reach_it(self):
        self.assertGreaterEqual(
            probe.DENIED_SERVED_WAIT_SECONDS, probe.POD_START_SECONDS + probe.SERVE_WAIT_SECONDS)

    def test_a_denied_pod_waits_through_the_start_and_one_recreate_of_the_control(self):
        # The other denied pod's start, the control's start, one recreate of the control, and the
        # control's first request: a retry the run budgets for must not fail the denied pod.
        start = probe.POD_START_SECONDS
        self.assertGreaterEqual(
            probe.DENIED_SERVED_WAIT_SECONDS,
            start + start + probe.START_BOUND_SECONDS + start + probe.SERVE_WAIT_SECONDS)

    def test_a_denied_pod_fits_its_own_pod_deadline(self):
        self.assertGreater(
            probe.POD_DEADLINE_SECONDS,
            probe.DENIED_SERVED_WAIT_SECONDS + probe.ENFORCEMENT_WINDOW_SECONDS
            + probe.HOLD_SECONDS + probe.LINGER_SECONDS)

    def test_the_install_instructions_wait_as_long_as_this_test_assumes(self):
        generator = (RUST.parent.parent / "generator.rs").read_text()
        self.assertIn(f"--timeout {INSTALL_WAIT_SECONDS // 60}m", generator)
        # Every place that installs or upgrades the chart waits as long.
        crates = RUST.parents[3]
        wait = f"{INSTALL_WAIT_SECONDS // 60}m"
        self.assertIn(f"--timeout {wait}", (crates / "alien-cli/src/commands/onboard.rs").read_text())
        self.assertIn(f'"--timeout", "{wait}"', (crates / "alien-deploy-cli/src/commands/sync.rs").read_text())
        self.assertIn(f"timeout: {wait}", generator)

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
