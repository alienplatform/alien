"""Exercise trusted provider identity mappings through the log verifier CLI."""

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class IdentityMap(unittest.TestCase):
    def test_provider_ids_remain_distinct_from_gateway_subjects(self):
        report = {
            "revision": "deployed-test-revision",
            "cases": [
                {
                    "name": "create-allow",
                    "subject": {
                        "sandboxId": "provider-id",
                        "startedAt": 100,
                        "finishedAt": 101,
                        "probe": {"url": "https://allowed.test/"},
                        "observation": {"outcome": "response"},
                    },
                }
            ],
        }
        event = {
            "message": "egress connection",
            "aud": "deployment/resource",
            "sub": "gateway-subject",
            "host": "allowed.test",
            "port": 443,
            "decision": "allow",
            "bytesUp": 1,
            "bytesDown": 1,
            "durationMs": 1,
            "ts": "1970-01-01T00:01:40.500000+00:00",
        }
        mapping = {
            "revision": report["revision"],
            "audience": event["aud"],
            "subjects": {"provider-id": "gateway-subject"},
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            report_file, logs, identities = (
                root / name for name in ("report.json", "logs.jsonl", "identities.json")
            )
            report_file.write_text(json.dumps(report))
            logs.write_text(json.dumps(event) + "\n")

            def invoke(identity_map, index):
                output = root / f"result-{index}.json"
                command = [
                    sys.executable,
                    str(Path(__file__).with_name("verify_logs.py")),
                    str(report_file),
                    str(logs),
                    "--audience",
                    event["aud"],
                    "--output",
                    str(output),
                ]
                if identity_map is not None:
                    identities.write_text(json.dumps(identity_map))
                    command.extend(["--identity-map", str(identities)])
                process = subprocess.run(
                    command, check=False, capture_output=True, text=True, timeout=10
                )
                result = json.loads(output.read_text()) if output.exists() else None
                return process, result

            process, result = invoke(None, "absent")
            self.assertEqual(1, process.returncode)
            self.assertEqual("unproven", result["verdict"])

            process, result = invoke(mapping, "valid")
            self.assertEqual(0, process.returncode, process.stderr)
            self.assertEqual("pass", result["verdict"])
            self.assertEqual("provider-id", result["connections"][0]["sandboxId"])
            self.assertEqual(
                "gateway-subject", result["connections"][0]["gatewaySubject"]
            )
            self.assertEqual(report, json.loads(report_file.read_text()))

            for index, subjects in enumerate(({}, {"provider-id": "wrong-subject"})):
                process, result = invoke(dict(mapping, subjects=subjects), index)
                self.assertEqual(1, process.returncode, process.stderr)
                self.assertEqual("unproven", result["verdict"])

            for index, change in enumerate(
                (
                    {"audience": "other/resource"},
                    {"revision": "old-revision"},
                    {"subjects": {"provider-id": ""}},
                    {"subjects": {"provider-id": None}},
                    {"subjects": ["gateway-subject"]},
                    {
                        "subjects": {
                            "provider-id": "gateway-subject",
                            "other-provider-id": "gateway-subject",
                        }
                    },
                )
            ):
                with self.subTest(change=change):
                    process, result = invoke(dict(mapping, **change), f"bad-{index}")
                    self.assertEqual(2, process.returncode, process.stderr)
                    self.assertIsNone(result)

            # An explicit map must not fall back to the provider ID when missing.
            logs.write_text(json.dumps(dict(event, sub="provider-id")) + "\n")
            process, result = invoke(dict(mapping, subjects={}), "no-fallback")
            self.assertEqual(1, process.returncode)
            self.assertEqual("unproven", result["verdict"])


if __name__ == "__main__":
    unittest.main()
