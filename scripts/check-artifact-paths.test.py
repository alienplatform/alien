#!/usr/bin/env python3
"""Run the artifact path check against fixture workflows."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parent / "check-artifact-paths.py"

# The step that uploaded the env file in the old end-to-end workflow.
OLD_E2E_UPLOAD = """
jobs:
  e2e:
    steps:
      - name: Upload .env.test artifact
        uses: actions/upload-artifact@v7
        with:
          name: env-test-e2e
          path: |
            .env.test
            .alien-kubeconfigs/
          if-no-files-found: error
          include-hidden-files: true
          retention-days: 1
"""


def step(uses, path, extra=""):
    return f"""
jobs:
  j:
    steps:
      - name: S
        uses: {uses}
        with:
          path: {path}
{extra}"""


class ArtifactPathsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def run_check(self):
        return subprocess.run(
            [sys.executable, "-I", str(SCRIPT), "--root", str(self.root)],
            capture_output=True, text=True,
        )

    def test_old_env_upload_fails(self):
        self.write(".github/workflows/e2e.yml", OLD_E2E_UPLOAD)
        result = self.run_check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("e2e.yml:Upload .env.test artifact", result.stdout)
        self.assertIn("`.env.test` matches `.env.*`", result.stdout)
        self.assertIn("`.alien-kubeconfigs/` matches `*kubeconfig*`", result.stdout)
        self.assertIn("include-hidden-files", result.stdout)

    def test_hidden_files_upload_fails(self):
        self.write(".github/workflows/a.yml", step("actions/upload-artifact@v4", "build/", "          include-hidden-files: true\n"))
        result = self.run_check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("include-hidden-files", result.stdout)

    def test_hidden_files_false_passes(self):
        self.write(".github/workflows/a.yml", step("actions/upload-artifact@v4", "build/", "          include-hidden-files: false\n"))
        self.assertEqual(self.run_check().returncode, 0)

    def test_good_upload_passes(self):
        self.write(".github/workflows/a.yml", step("actions/upload-artifact@v4", "target/release/tool"))
        result = self.run_check()
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_cache_and_composite_action_are_checked(self):
        self.write(".github/workflows/a.yml", step("actions/cache@v4", "~/.kube/"))
        self.write(
            ".github/actions/x/action.yml",
            "runs:\n  using: composite\n  steps:\n    - uses: actions/cache/save@v4\n      with:\n        path: certs/server.pem\n",
        )
        result = self.run_check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("a.yml:S", result.stdout)
        self.assertIn("actions/x/action.yml:step-1", result.stdout)

    def test_each_pattern(self):
        for bad in [".env", ".env.prod", "ci.env", "my-kubeconfig", "a/.kube/config", "k.pem", "k.p12",
                    "service-key.json", "aws-credentials", "terraform.tfstate.backup", "id_rsa.pub", "A/.ENV"]:
            with self.subTest(bad=bad):
                self.write(".github/workflows/a.yml", step("actions/upload-artifact@v4", f"'{bad}'"))
                self.assertEqual(self.run_check().returncode, 1)

    def test_broad_paths_fail(self):
        for bad in [".", "./", "'*'", "'**'", "~", "~/", "./**", "${{ github.workspace }}", "${{ github.workspace }}/**"]:
            with self.subTest(bad=bad):
                self.write(".github/workflows/a.yml", step("actions/cache@v4", f"\"{bad}\"" if "'" not in bad else bad))
                result = self.run_check()
                self.assertEqual(result.returncode, 1, result.stdout)
                self.assertIn("whole checkout or home directory", result.stdout)

    def test_negated_and_plain_paths_pass(self):
        self.write(".github/workflows/a.yml", step("actions/cache@v4", "|\n            dist/\n            !dist/.env\n            node_modules"))
        self.assertEqual(self.run_check().returncode, 0)

    def test_allow_list(self):
        self.write(".github/workflows/e2e.yml", OLD_E2E_UPLOAD)
        self.write(".github/artifact-allowlist.txt", "# c\ne2e.yml:Upload .env.test artifact | test fixture\n")
        self.assertEqual(self.run_check().returncode, 0)

    def test_allow_list_needs_reason(self):
        self.write(".github/workflows/a.yml", step("actions/upload-artifact@v4", "x"))
        self.write(".github/artifact-allowlist.txt", "a.yml:S\n")
        self.assertNotEqual(self.run_check().returncode, 0)


if __name__ == "__main__":
    unittest.main()
