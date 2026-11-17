"""The probe's time budget: its parts add up and fit the Job and the install's wait."""

import pathlib
import re
import unittest

from loader import probe

RUST = pathlib.Path(__file__).parents[2] / "src" / "emitters" / "sandbox_cni.rs"
HELM_TIMEOUT_SECONDS = 900  # `--timeout 15m`, applied by Helm to each hook


class Budget(unittest.TestCase):
    def test_the_script_budget_is_the_sum_of_its_parts(self):
        start = probe.POD_START_SECONDS
        total = (probe.API_READY_SECONDS + probe.ADMISSION_SECONDS
                 + probe.PENDING_CAUSE_BOUND_SECONDS                  # listener (a node may be provisioned)
                 + start + probe.CLIENT_RUN_SECONDS                   # stage 1
                 + start + probe.RECOVER_RUN_SECONDS                  # stage 2
                 + probe.START_BOUND_SECONDS + start                  # one recreate
                 + probe.CLEANUP_SECONDS)
        self.assertEqual(probe.SCRIPT_BUDGET_SECONDS, total)

    def test_the_stages_the_budget_counts_are_the_stages_that_run(self):
        self.assertEqual(len(probe.STAGES), 2)

    def test_the_job_deadline_covers_the_script_the_runner_start_and_a_margin(self):
        self.assertEqual(
            probe.JOB_DEADLINE_SECONDS,
            probe.SCRIPT_BUDGET_SECONDS + probe.RUNNER_START_SECONDS + probe.BUDGET_MARGIN_SECONDS)

    def test_the_job_in_the_chart_uses_that_deadline(self):
        job = re.search(r"kind: Job.*?activeDeadlineSeconds: (\d+|\{[^\n]*)", RUST.read_text(), re.S)
        self.assertIsNotNone(job)
        self.assertNotRegex(job.group(1), r"^\d+$", "the Job's deadline is read from the script, not typed twice")

    def test_the_job_fits_inside_helms_timeout_for_a_hook(self):
        self.assertLessEqual(probe.JOB_DEADLINE_SECONDS, HELM_TIMEOUT_SECONDS)

    def test_a_pending_pod_waits_at_least_as_long_as_the_install_did_before(self):
        self.assertGreaterEqual(probe.PENDING_CAUSE_BOUND_SECONDS, 210)

    def test_a_client_pods_own_deadline_covers_its_longest_run(self):
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
