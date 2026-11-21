#!/usr/bin/env python3
"""Tests for affected-rust-packages.py. Run: python3 scripts/affected-rust-packages.test.py

The selection runs on synthetic file lists and a synthetic package graph; the last tests check
that every entry of scripts/ci-checks.json still names things that exist in the repository.
"""

import importlib.util
import pathlib
import re
import unittest
from pathlib import PurePosixPath

HERE = pathlib.Path(__file__).parent
ROOT = HERE.parent
_spec = importlib.util.spec_from_file_location("affected", HERE / "affected-rust-packages.py")
affected = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(affected)
CONFIG = affected.load_config()

PACKAGES = {
    name: PurePosixPath("crates") / name
    for name in (
        "alien-core",
        "alien-cli",
        "alien-permissions",
        "alien-preflights",
        "alien-terraform",
        "alien-cloudformation",
        "alien-helm",
        "alien-client-core",
    )
}
DEPENDENTS = {name: set() for name in PACKAGES}
for dependent in ("alien-preflights", "alien-terraform", "alien-cloudformation", "alien-helm"):
    DEPENDENTS["alien-permissions"].add(dependent)
for name in PACKAGES:
    if name not in ("alien-core", "alien-client-core"):
        DEPENDENTS["alien-core"].add(name)

TS = {
    "@alienplatform/typescript-config": {"dir": PurePosixPath("packages/typescript-config"), "deps": set()},
    "@alienplatform/core": {"dir": PurePosixPath("packages/core"), "deps": {"@alienplatform/typescript-config"}},
    "@alienplatform/permissions": {"dir": PurePosixPath("packages/permissions"), "deps": set()},
    "@alienplatform/commands": {"dir": PurePosixPath("packages/commands"), "deps": {"@alienplatform/core"}},
    "@alienplatform/ai-gateway": {"dir": PurePosixPath("packages/ai-gateway"), "deps": {"@alienplatform/core"}},
}


def select(files, own_only=True):
    return affected.select(files, PACKAGES, DEPENDENTS, CONFIG, own_only, TS)


class Selection(unittest.TestCase):
    def test_documentation_selects_nothing(self):
        result = select(["docs/guide.mdx", "README.md"])
        self.assertEqual((result["mode"], result["packages"], result["repo_wide"], result["ts_filters"]), ("none", "", "", ""))

    def test_a_plain_cli_change_selects_its_own_package_and_the_repo_wide_checks(self):
        result = select(["crates/alien-cli/src/main.rs"])
        self.assertEqual(result["packages"], "-p alien-cli -p alien-client-core")
        self.assertEqual(result["test_filter"], "")
        self.assertIn("install-wait scan", result["repo_wide"])

    def test_a_permissions_change_selects_the_output_checks_of_its_dependents_only(self):
        result = select(["crates/alien-permissions/permission-sets/vault/data-write.jsonc"])
        for name in ("alien-preflights", "alien-terraform", "alien-cloudformation", "alien-helm"):
            self.assertIn(f"-p {name}", result["packages"])
        self.assertIn("binary(aws_manual_permissions_compatibility)", result["test_filter"])
        self.assertIn("package(alien-permissions) |", result["test_filter"])
        # Their whole suites are not selected: no bare package term for a dependent.
        self.assertNotIn("package(alien-preflights) |", result["test_filter"])
        self.assertTrue(result["features"].find("alien-preflights/runtime-checks") >= 0)

    def test_a_changed_dependent_keeps_its_whole_suite(self):
        result = select(["crates/alien-permissions/src/lib.rs", "crates/alien-helm/src/lib.rs"])
        self.assertIn("package(alien-helm) |", result["test_filter"] + " |")
        self.assertNotIn("(package(alien-helm) &", result["test_filter"])

    def test_alien_core_is_not_expanded_in_own_only_mode(self):
        result = select(["crates/alien-core/src/lib.rs"])
        self.assertEqual(result["packages"], "-p alien-client-core -p alien-core")

    def test_a_full_run_selects_every_package_without_a_filter(self):
        result = select(["crates/alien-core/src/lib.rs"], own_only=False)
        self.assertEqual(result["mode"], "affected")
        self.assertEqual(result["test_filter"], "")
        self.assertEqual(result["count"], len(PACKAGES))

    def test_a_file_outside_the_packages_is_check_only(self):
        result = select(["Cargo.lock"])
        self.assertEqual(result["mode"], "check-only")
        self.assertEqual(result["packages"], "")
        self.assertIn("install-wait scan", result["repo_wide"])


class TypeScript(unittest.TestCase):
    def test_a_generated_client_change_selects_core(self):
        self.assertEqual(select(["client-sdks/platform/typescript/src/sdk/deployments.ts"])["ts_filters"], "--filter @alienplatform/core")

    def test_an_openapi_spec_selects_core(self):
        self.assertEqual(select(["crates/alien-manager/openapi.json"])["ts_filters"], "--filter @alienplatform/core")

    def test_a_package_selects_itself_and_the_packages_that_depend_on_it(self):
        filters = select(["packages/core/src/index.ts"])["ts_filters"]
        self.assertEqual(filters, "--filter @alienplatform/core --filter @alienplatform/commands --filter @alienplatform/ai-gateway")

    def test_a_dependency_of_core_selects_core(self):
        self.assertIn("@alienplatform/core", select(["packages/typescript-config/base.json"])["ts_filters"])

    def test_an_unrelated_package_selects_nothing(self):
        self.assertEqual(select(["packages/other/src/index.ts"])["ts_filters"], "")


class ConfigStillMatchesTheRepository(unittest.TestCase):
    def test_every_dependents_entry_names_a_real_dependent_and_test_target(self):
        for changed, entries in CONFIG["dependents"].items():
            for entry in entries:
                manifest = (ROOT / "crates" / entry["package"] / "Cargo.toml").read_text()
                self.assertRegex(manifest, rf"(?m)^{re.escape(changed)}\s*=", f"{entry['package']} does not depend on {changed}")
                self.assertTrue(entry["why"])
                for target in re.findall(r"binary\((\w+)\)", entry["filter"]):
                    if target == "generator":
                        continue
                    self.assertTrue(
                        (ROOT / "crates" / entry["package"] / "tests" / f"{target}.rs").exists(),
                        f"{entry['package']} has no test target {target}",
                    )
                for module in re.findall(r"test\((\w+)\)", entry["filter"]):
                    found = list((ROOT / "crates" / entry["package"] / "tests").rglob(f"{module}.rs"))
                    self.assertTrue(found, f"{entry['package']} has no test module {module}")

    def test_every_repo_wide_check_has_a_reason_and_a_real_directory(self):
        for check in CONFIG["repo_wide"]:
            self.assertTrue(check["why"])
            self.assertTrue((ROOT / check["cwd"]).is_dir(), check["cwd"])

    def test_the_typescript_packages_exist_in_the_workspace(self):
        workspace = affected.load_ts_packages(PurePosixPath(ROOT))
        for name in CONFIG["typescript"]["test_packages"]:
            self.assertIn(name, workspace)


if __name__ == "__main__":
    unittest.main()
