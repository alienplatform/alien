"""The probe's time budget is stated once; the chart, the pods and the install waits follow it."""

import pathlib
import re
import unittest

from loader import probe

CRATES = pathlib.Path(__file__).parents[3]
RUST = CRATES / "alien-helm/src/emitters/sandbox_cni.rs"
INSTALL_WAIT_SECONDS = 900  # `--timeout 15m`, applied by Helm to each hook
NEXT_HOOK_SECONDS = 120


class Budget(unittest.TestCase):
    def test_the_script_budget_is_the_sum_of_its_parts(self):
        self.assertEqual(
            probe.SCRIPT_BUDGET_SECONDS,
            30 + 210 + (35 + 15) + (35 + 30 + 5) + 45 + 35 + 10)

    def test_the_job_deadline_is_the_script_the_runner_wait_and_a_margin(self):
        self.assertEqual(
            probe.JOB_DEADLINE_SECONDS,
            probe.API_READY_SECONDS + probe.SCRIPT_BUDGET_SECONDS
            + probe.RUNNER_START_SECONDS + probe.BUDGET_MARGIN_SECONDS)

    def test_the_chart_reads_the_job_deadline_from_the_script(self):
        job = re.search(r"kind: Job.*?activeDeadlineSeconds: (\S+)", RUST.read_text(), re.S)
        self.assertEqual(job.group(1), "{job_deadline}")

    def test_the_job_leaves_the_next_hook_room_inside_the_install_wait(self):
        self.assertLessEqual(probe.JOB_DEADLINE_SECONDS + NEXT_HOOK_SECONDS, INSTALL_WAIT_SECONDS)

    def test_every_install_path_waits_as_long_as_the_budget_assumes(self):
        wait = f"{INSTALL_WAIT_SECONDS // 60}m"
        generator = (CRATES / "alien-helm/src/generator.rs").read_text()
        self.assertIn(f"--timeout {wait}", generator)          # instructions
        self.assertIn(f"timeout: {wait}", generator)            # Flux HelmRelease
        self.assertNotIn("--timeout 10m", generator)
        self.assertIn(f"--timeout {wait}", (CRATES / "alien-cli/src/commands/onboard.rs").read_text())
        self.assertIn(f'"--timeout", "{wait}"', (CRATES / "alien-deploy-cli/src/commands/sync.rs").read_text())
        self.assertIn(f'.arg("--timeout")\n        .arg("{wait}")',
                      (CRATES / "alien-deploy-cli/src/commands/up.rs").read_text())
        self.assertIn(f"--timeout {wait}",
                      (CRATES.parent / "examples/customer-kubernetes/README.md").read_text())

    def test_every_pod_outlives_the_whole_run(self):
        self.assertIn('"activeDeadlineSeconds": SCRIPT_BUDGET_SECONDS',
                      pathlib.Path(probe.__file__).read_text())

    def test_a_listener_may_wait_as_long_as_the_install_did_before(self):
        self.assertGreaterEqual(probe.LISTENER_SECONDS, 210)

    def test_three_kata_starts_in_two_stages_fit_the_stages(self):
        # The stages start the clients of a stage together: two pod starts, not five.
        self.assertEqual(len(probe.STAGES), 2)
        self.assertGreaterEqual(probe.STAGE_ONE_SECONDS + probe.STAGE_TWO_SECONDS,
                                2 * probe.POD_START_SECONDS)


if __name__ == "__main__":
    unittest.main()
