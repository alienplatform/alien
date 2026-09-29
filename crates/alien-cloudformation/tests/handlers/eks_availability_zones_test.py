"""Exercise the embedded handler's AWS and CloudFormation response contract."""
import importlib.util
import json
import sys
import unittest
from unittest.mock import MagicMock, patch

sys.dont_write_bytecode = True
boto3 = MagicMock()
sys.modules["boto3"] = boto3
sys.modules["botocore"] = MagicMock()
sys.modules["botocore.config"] = MagicMock()
spec = importlib.util.spec_from_file_location("lookup", sys.argv.pop(1))
lookup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lookup)


class LookupTests(unittest.TestCase):
    def setUp(self):
        boto3.reset_mock()
        lookup.Config.reset_mock()
        boto3.client.return_value.describe_availability_zones.side_effect = None
        self.event = {
            "RequestType": "Create", "StackId": "stack", "LogicalResourceId": "Lookup",
            "RequestId": "request", "ResponseURL": "https://response.example.test",
            "ResourceProperties": {"RequestedCount": "2", "ExcludedZoneIds": ["use1-az3"]},
        }
        self.zones = [self.zone("use1-az4", "us-east-1a"), self.zone("use1-az3", "us-east-1b"),
                      self.zone("use1-az1", "us-east-1c")]
        boto3.client.return_value.describe_availability_zones.return_value = {"AvailabilityZones": self.zones}

    @staticmethod
    def zone(zone_id, name, state="available", zone_type="availability-zone"):
        return {"ZoneId": zone_id, "ZoneName": name, "State": state, "ZoneType": zone_type}

    def invoke(self):
        with patch.object(lookup.urllib.request, "urlopen") as http:
            lookup.handler(self.event, None)
            request = http.call_args.args[0]
            self.assertEqual(request.get_method(), "PUT")
            self.assertEqual(request.full_url, self.event["ResponseURL"])
            self.assertEqual(int(request.get_header("Content-length")), len(request.data))
            self.assertEqual(http.call_args.kwargs["timeout"], 20)
            return json.loads(request.data)

    def test_create_filters_physical_ids_and_sorts_account_local_names(self):
        self.zones.extend([self.zone("use1-az0", "local", zone_type="local-zone"),
                           self.zone("use1-az2", "disabled", state="unavailable"), self.zones[0]])
        response = self.invoke()
        self.assertEqual(response["Status"], "SUCCESS")
        lookup.Config.assert_called_once_with(connect_timeout=5, read_timeout=10,
                                             retries={"mode": "standard", "total_max_attempts": 1})
        boto3.client.assert_called_once_with("ec2", config=lookup.Config.return_value)
        self.assertEqual(response["Data"], {"ZoneIds": ["use1-az1", "use1-az4"], "ZoneNames": ["us-east-1c", "us-east-1a"]})
        boto3.client.return_value.describe_availability_zones.assert_called_once_with(
            Filters=[{"Name": "state", "Values": ["available"]}, {"Name": "zone-type", "Values": ["availability-zone"]}],
            AllAvailabilityZones=False,
        )

    def test_update_keeps_physical_identity(self):
        self.event.update(RequestType="Update", PhysicalResourceId="previous")
        self.assertEqual(self.invoke()["PhysicalResourceId"], "previous")

    def test_delete_never_queries_aws(self):
        self.event.update(RequestType="Delete", PhysicalResourceId="previous")
        self.event.pop("ResourceProperties")
        response = self.invoke()
        self.assertEqual(response["Status"], "SUCCESS")
        self.assertEqual(response["Data"], {})
        boto3.client.assert_not_called()

    def test_insufficient_or_invalid_input_returns_failed_response(self):
        for count in ["3", "1", "invalid"]:
            with self.subTest(count=count):
                self.event["ResourceProperties"]["RequestedCount"] = count
                self.assertEqual(self.invoke()["Status"], "FAILED")

    def test_duplicate_names_are_not_distinct_zones(self):
        self.zones[2]["ZoneName"] = self.zones[0]["ZoneName"]
        self.assertEqual(self.invoke()["Status"], "FAILED")

    def test_aws_failure_still_responds(self):
        boto3.client.return_value.describe_availability_zones.side_effect = RuntimeError("access denied")
        response = self.invoke()
        self.assertEqual(response["Status"], "FAILED")
        self.assertIn("access denied", response["Reason"])

    def test_response_failure_is_not_swallowed(self):
        with patch.object(lookup.urllib.request, "urlopen", side_effect=TimeoutError("response timeout")):
            with self.assertRaises(TimeoutError):
                lookup.handler(self.event, None)


unittest.main()
