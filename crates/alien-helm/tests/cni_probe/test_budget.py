"""The probe's time budget is stated once; the chart, the pods and the install waits follow it."""

import pathlib
import re
import subprocess
import unittest

from loader import probe

CRATES = pathlib.Path(__file__).parents[3]
RUST = CRATES / "alien-helm/src/emitters/sandbox_cni.rs"
INSTALL_WAIT_SECONDS = 900  # `--timeout 15m`, applied by Helm to each hook
NEXT_HOOK_SECONDS = 120
ROOT = CRATES.parent
SUFFIXES = {".rs", ".sh", ".md", ".tf", ".py", ".yml", ".yaml", ".mdx", ".tpl"}
# A file that runs `helm install` / `helm upgrade`, or a Terraform `helm_release`.
INSTALLS = re.compile(r'helm (install|upgrade)\b|Command::new\("helm"\)|"helm_release"|kind: HelmRelease|"helm",\s*$', re.M)
# The 15 minute wait, in each spelling an install path uses.
WAITS = re.compile(r'--timeout[ =",\s]+15m|timeout: 15m|Number\(900|"15m"|timeout = 900')
# Files that match INSTALLS but do not install a chart whose setup check can take that long: they
# only mention a command, uninstall, render, or install a chart with no sandbox or container
# egress (no setup check runs). A path that is not here must state the 15 minute wait.
NO_SETUP_CHECK = {
    ".github/workflows/kind-lifecycle.yml": "installs the CNI (cilium), not the chart",
    "crates/alien-cli/README.md": "describes the command",
    "crates/alien-core/src/deployment/state.rs": "doc comment",
    "crates/alien-core/src/extension.rs": "names the setup format",
    "crates/alien-deploy-cli/README.md": "describes the command",
    "crates/alien-deploy-cli/src/commands/helm_release.rs": "adds the release check to the install sync.rs builds with the wait; its tests build commands",
    "crates/alien-deploy-cli/src/commands/register.rs": "name of the import source",
    "crates/alien-deployment/src/initial_setup.rs": "error text",
    "crates/alien-deployment/src/manager_api_transport.rs": "the setup method name",
    "crates/alien-deployment/tests/helm_initial_setup_retry.rs": "error text",
    "crates/alien-extension-sdk/tests/release_check.rs": "renders, does not install",
    "crates/alien-helm/examples/render_chart.rs": "renders, does not install",
    "crates/alien-helm/src/package.rs": "comment",
    "crates/alien-helm/src/templates/_runtime-identity.tpl": "comment",
    "crates/alien-helm/src/test_utils.rs": "renders, does not install",
    "crates/alien-helm/tests/generator/resource_layer_tests.rs": "renders, does not install",
    "crates/alien-helm/tests/product_remote_operator_lifecycle.rs": "chart without sandbox or container egress: no setup check",
    "crates/alien-helm/tests/runtime_identity_lifecycle.rs": "chart without sandbox or container egress: no setup check",
    "crates/alien-helm/tests/signing_kind.rs": "chart without sandbox or container egress: no setup check",
    "crates/alien-helm/src/operator_chart.rs": "the Operator chart's README text; the chart has no setup check",
    "crates/alien-helm/tests/operator_chart.rs": "helm template and lint only, no install",
    "crates/alien-infra/src/service_account/kubernetes.rs": "comment",
    "crates/alien-infra/tests/kubernetes_service_account_refresh_kind.rs": "comment",
    "crates/alien-manager/src/routes/charts.rs": "serves the chart",
    "crates/alien-manager/src/routes/installed_setup.rs": "comment",
    "crates/alien-manager/src/routes/sync.rs": "comment",
    "crates/alien-manager/tests/handoff_lock_routes.rs": "the setup method name",
    "crates/alien-manager/tests/stack_import/protocol.rs": "names the import source",
    "crates/alien-operator/src/cli.rs": "error text and comments naming the helm commands; it runs none",
    "crates/alien-operator/src/daily_identity.rs": "comment",
    "crates/alien-operator/src/identity_store.rs": "error text and comments",
    "crates/alien-operator/src/loops/sync.rs": "comment",
    "crates/alien-operator/src/self_update.rs": "comment",
    "crates/alien-preflights/src/compatibility/frozen_resources_unchanged.rs": "error text",
    "crates/alien-signing/src/release.rs": "comment",
    "crates/alien-terraform/tests/erase_vault_data.rs": "Terraform state fixture",
    "crates/alien-terraform/tests/generator/k8s_overlay_tests.rs": "asserts on generated text",
    "crates/alien-test/README.md": "describes the command",
    "crates/alien-test/src/cleanup.rs": "uninstalls",
    "crates/alien-test/src/operator/diagnostics.rs": "comment",
    "infra/README.md": "the manager's own chart: no sandbox",
    "scripts/setup-render-diff/renderer/src/main.rs": "renders, does not install",
    "scripts/signed-release-e2e/operator_kind.py": "comment",
    "tests/e2e/helm-setup-update/README.md": "describes the case",
}


def tracked_files():
    listing = subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True, check=True)
    return [name for name in listing.stdout.split("\n")
            if pathlib.Path(name).suffix in SUFFIXES and not name.startswith("client-sdks/")]


class Budget(unittest.TestCase):
    def test_the_script_budget_is_the_sum_of_its_parts(self):
        self.assertEqual(
            probe.SCRIPT_BUDGET_SECONDS,
            30 + (210 + 35) + 2 * (35 + 15) + (35 + 120 + 5) + 45 + 35 + 10)

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

    def test_every_install_path_in_the_tree_waits_as_long_as_the_budget_assumes(self):
        # Found by searching the tree, so a new install path cannot be forgotten.
        missing = []
        for name in tracked_files():
            if name == "crates/alien-helm/tests/cni_probe/test_budget.py":
                continue
            text = (ROOT / name).read_text(errors="ignore")
            if INSTALLS.search(text) and not WAITS.search(text) and name not in NO_SETUP_CHECK:
                missing.append(name)
        self.assertEqual(missing, [], "these files install or upgrade a chart without the 15 minute wait; "
                         "add `--timeout 15m` (or its equivalent), or list them in NO_SETUP_CHECK with the reason")

    def test_the_exception_list_has_no_stale_entries(self):
        tracked = set(tracked_files())
        for name in NO_SETUP_CHECK:
            self.assertIn(name, tracked)
            self.assertRegex((ROOT / name).read_text(errors="ignore"), INSTALLS, name)

    def test_the_known_install_paths_state_the_wait(self):
        wait = "15m"
        generator = (CRATES / "alien-helm/src/generator.rs").read_text()
        self.assertIn(f"--timeout {wait}", generator)          # instructions
        self.assertIn(f"timeout: {wait}", generator)            # Flux HelmRelease
        self.assertIn("timeout = 900", "timeout = %d" % INSTALL_WAIT_SECONDS)
        self.assertIn("Number(900.into())", (CRATES / "alien-terraform/src/generator.rs").read_text())

    def test_a_message_names_the_budget_it_quotes(self):
        # The comments that quote the budget follow it.
        sync = (CRATES / "alien-deploy-cli/src/commands/sync.rs").read_text()
        self.assertIn(f"budget is {probe.SCRIPT_BUDGET_SECONDS} s, {probe.JOB_DEADLINE_SECONDS} s for its Job", sync)
        minutes = -(-probe.JOB_DEADLINE_SECONDS // 60)
        self.assertIn(f"waits up to {minutes} minutes", (CRATES / "alien-helm/src/generator.rs").read_text())

    def test_every_pod_outlives_the_whole_run(self):
        self.assertIn('"activeDeadlineSeconds": SCRIPT_BUDGET_SECONDS',
                      pathlib.Path(probe.__file__).read_text())

    def test_the_listener_may_wait_for_a_node_that_a_pool_scaling_from_zero_provisions(self):
        # 120 to 210 s for the node, then the pod starts.
        self.assertGreaterEqual(probe.LISTENER_SECONDS, 210 + probe.POD_START_SECONDS)

    def test_a_client_may_wait_as_long_as_the_listener(self):
        self.assertGreaterEqual(probe.PENDING_SECONDS, probe.LISTENER_SECONDS)

    def test_the_stages_run_the_control_alone_first(self):
        self.assertEqual([[role for role, _, _ in stage] for stage in probe.STAGES],
                         [["control"], ["denied", "ingress-denied"], ["control-after", "ingress-control-after"]])

    def test_the_control_after_has_the_recovery_window_main_had(self):
        self.assertEqual(dict((role, recover) for stage in probe.STAGES for role, _, recover in stage)
                         ["ingress-control-after"], 120)


if __name__ == "__main__":
    unittest.main()
