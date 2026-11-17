"""The probe's time budget: its parts add up and fit the Job and the install's wait."""

import pathlib
import re
import unittest

from loader import probe

RUST = pathlib.Path(__file__).parents[2] / "src" / "emitters" / "sandbox_cni.rs"
INSTALL_WAIT_SECONDS = 600  # `helm install --timeout 10m`
NEXT_HOOK_SECONDS = 120  # the root boot check's own pod deadline


class Budget(unittest.TestCase):
    def test_the_script_budget_is_the_sum_of_its_parts(self):
        start = probe.POD_START_SECONDS
        total = (probe.API_READY_SECONDS + probe.ADMISSION_SECONDS
                 + start                                              # listener
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

    def test_the_job_leaves_the_next_hook_room_inside_the_install_wait(self):
        self.assertLessEqual(probe.JOB_DEADLINE_SECONDS + NEXT_HOOK_SECONDS, INSTALL_WAIT_SECONDS)

    def test_a_client_pods_own_deadline_covers_its_longest_run(self):
        self.assertGreaterEqual(
            probe.POD_DEADLINE_SECONDS, probe.POD_START_SECONDS + probe.RECOVER_RUN_SECONDS)

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
