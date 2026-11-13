"""Exercise the embedded replication handler: foreign rules survive every request type."""
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
spec = importlib.util.spec_from_file_location("rule", sys.argv.pop(1))
rule = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rule)

OURS = {
    "destinations": [{"region": "us-east-2", "registryId": "111111111111"}],
    "repositoryFilters": [{"filter": "stack-registry", "filterType": "PREFIX_MATCH"}],
}
FOREIGN = [
    {  # another registry's rule
        "destinations": [{"region": "eu-west-1", "registryId": "111111111111"}],
        "repositoryFilters": [{"filter": "other-registry", "filterType": "PREFIX_MATCH"}],
    },
    {  # a rule that sends images to a different account
        "destinations": [{"region": "us-west-2", "registryId": "222222222222"}],
        "repositoryFilters": [{"filter": "stack-registry", "filterType": "PREFIX_MATCH"},
                              {"filter": "extra", "filterType": "PREFIX_MATCH"}],
    },
    {"destinations": [{"region": "us-west-1", "registryId": "111111111111"}]},
]


class RuleTests(unittest.TestCase):
    def setUp(self):
        boto3.reset_mock()
        self.ecr = boto3.client.return_value
        self.ecr.describe_registry.side_effect = None
        self.ecr.describe_registry.return_value = {"replicationConfiguration": {"rules": []}}
        self.event = {
            "RequestType": "Create", "StackId": "stack", "LogicalResourceId": "Rule",
            "RequestId": "request", "ResponseURL": "https://response.example.test",
            "ResourceProperties": {"Prefix": "stack-registry", "RegistryId": "111111111111",
                                   "Regions": ["us-east-2"]},
        }

    def invoke(self):
        with patch.object(rule.urllib.request, "urlopen") as http:
            rule.handler(self.event, None)
            return json.loads(http.call_args.args[0].data)

    def written(self):
        self.ecr.put_replication_configuration.assert_called_once()
        return self.ecr.put_replication_configuration.call_args.kwargs[
            "replicationConfiguration"]["rules"]

    def test_create_keeps_every_foreign_rule_and_adds_ours(self):
        self.ecr.describe_registry.return_value = {"replicationConfiguration": {"rules": FOREIGN}}
        self.assertEqual(self.invoke()["Status"], "SUCCESS")
        self.assertEqual(self.written(), FOREIGN + [OURS])

    def test_update_replaces_only_our_rule(self):
        changed = dict(OURS, destinations=[{"region": "us-west-2", "registryId": "111111111111"}])
        self.ecr.describe_registry.return_value = {
            "replicationConfiguration": {"rules": [FOREIGN[0], changed, FOREIGN[1]]}}
        self.event["RequestType"] = "Update"
        self.assertEqual(self.invoke()["Status"], "SUCCESS")
        self.assertEqual(self.written(), [FOREIGN[0], FOREIGN[1], OURS])

    def test_update_with_a_new_prefix_removes_the_old_rule(self):
        old = dict(OURS, repositoryFilters=[{"filter": "old-registry", "filterType": "PREFIX_MATCH"}])
        self.ecr.describe_registry.return_value = {
            "replicationConfiguration": {"rules": [old, FOREIGN[0]]}}
        self.event.update(RequestType="Update", OldResourceProperties={"Prefix": "old-registry"})
        self.assertEqual(self.invoke()["Status"], "SUCCESS")
        self.assertEqual(self.written(), [FOREIGN[0], OURS])

    def test_delete_removes_only_our_rule(self):
        self.ecr.describe_registry.return_value = {
            "replicationConfiguration": {"rules": [FOREIGN[0], OURS, FOREIGN[1]]}}
        self.event["RequestType"] = "Delete"
        self.assertEqual(self.invoke()["Status"], "SUCCESS")
        self.assertEqual(self.written(), [FOREIGN[0], FOREIGN[1]])

    def test_delete_with_no_rule_of_ours_writes_nothing(self):
        self.ecr.describe_registry.return_value = {"replicationConfiguration": {"rules": FOREIGN}}
        self.event["RequestType"] = "Delete"
        self.assertEqual(self.invoke()["Status"], "SUCCESS")
        self.ecr.put_replication_configuration.assert_not_called()

    def test_unchanged_rules_are_not_rewritten(self):
        self.ecr.describe_registry.return_value = {"replicationConfiguration": {"rules": [OURS]}}
        self.assertEqual(self.invoke()["Status"], "SUCCESS")
        self.ecr.put_replication_configuration.assert_not_called()

    def test_empty_regions_remove_our_rule_and_keep_the_rest(self):
        self.ecr.describe_registry.return_value = {
            "replicationConfiguration": {"rules": [OURS, FOREIGN[0]]}}
        self.event["ResourceProperties"]["Regions"] = []
        self.assertEqual(self.invoke()["Status"], "SUCCESS")
        self.assertEqual(self.written(), [FOREIGN[0]])

    def test_a_bad_region_or_failed_read_fails_without_writing(self):
        self.event["ResourceProperties"]["Regions"] = ["too-many-regions-at-most-10-are-supported"]
        self.assertEqual(self.invoke()["Status"], "FAILED")
        self.event["ResourceProperties"]["Regions"] = ["us-east-2"]
        self.ecr.describe_registry.side_effect = RuntimeError("AccessDenied")
        response = self.invoke()
        self.assertEqual(response["Status"], "FAILED")
        self.assertIn("AccessDenied", response["Reason"])
        self.ecr.put_replication_configuration.assert_not_called()

    def test_a_rule_on_our_prefix_that_copies_to_another_account_is_kept_on_delete(self):
        foreign_same_prefix = dict(OURS, destinations=[{"region": "us-east-2", "registryId": "999999999999"}])
        self.ecr.describe_registry.return_value = {
            "replicationConfiguration": {"rules": [foreign_same_prefix, OURS]}}
        self.event["RequestType"] = "Delete"
        self.assertEqual(self.invoke()["Status"], "SUCCESS")
        self.assertEqual(self.written(), [foreign_same_prefix])

    def test_duplicate_regions_are_written_once(self):
        self.event["ResourceProperties"]["Regions"] = ["us-east-2", "us-east-2"]
        self.invoke()
        self.assertEqual(self.written(), [OURS])


if __name__ == "__main__":
    unittest.main()
