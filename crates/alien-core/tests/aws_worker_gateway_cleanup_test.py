"""Exercise the cleanup capability against botocore's actual ECS wire models."""
import importlib.util
import json
import os
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import boto3
from botocore.stub import Stubber

source = Path(__file__).parents[1] / "src/aws_worker_gateway/cleanup.py"
spec = importlib.util.spec_from_file_location("gateway_cleanup", source)
cleanup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cleanup)

PREFIX = "test-deployment"
TOKEN = "ad17aca8-05d9-4b32-893f-5d6f718a9551"
ARN = f"arn:aws:ecs:us-east-1:123456789012:task-definition/{PREFIX}-api-gateway-{TOKEN}:3"
CONTEXT = SimpleNamespace(
    invoked_function_arn="arn:aws:lambda:us-east-1:123456789012:function:setup-gateway-cleanup:7"
)
TAGS = {"deployment": PREFIX, "resource": "api", "managed-by": "runtime", "CreateAttempt": TOKEN}


class CleanupTests(unittest.TestCase):
    def setUp(self):
        self.ecs = boto3.client("ecs", region_name="us-east-1",
                                aws_access_key_id="test", aws_secret_access_key="test")
        self.stub = Stubber(self.ecs)
        self.stub.activate()
        self.addCleanup(self.stub.deactivate)

    def tags(self, values=None):
        values = TAGS if values is None else values
        self.stub.add_response("list_tags_for_resource", {
            "tags": [{"key": key, "value": value} for key, value in values.items()]
        }, {"resourceArn": ARN})

    def retire(self, action="delete", arn=ARN, context=CONTEXT):
        return cleanup.retire({"action": action, "taskDefinitionArn": arn}, context, PREFIX, self.ecs)

    def test_deregister_returns_identity_without_task_contents(self):
        self.tags()
        self.stub.add_response("deregister_task_definition", {"taskDefinition": {
            "taskDefinitionArn": ARN, "status": "INACTIVE", "containerDefinitions": [
                {"name": "gateway", "environment": [{"name": "SECRET", "value": "never-return-this"}]}
            ],
        }}, {"taskDefinition": ARN})
        self.assertEqual(self.retire("deregister"), {"taskDefinitionArn": ARN, "status": "INACTIVE"})
        self.stub.assert_no_pending_responses()

    def test_delete_acceptance_is_not_absence(self):
        self.tags()
        self.stub.add_response("delete_task_definitions", {"taskDefinitions": [
            {"taskDefinitionArn": ARN, "status": "DELETE_IN_PROGRESS"}
        ], "failures": []}, {"taskDefinitions": [ARN]})
        self.assertEqual(self.retire(), {"taskDefinitionArn": ARN, "status": "DELETE_IN_PROGRESS"})
        self.stub.assert_no_pending_responses()

    def test_wrong_boundary_is_refused_before_any_provider_read(self):
        for arn in [ARN.replace("123456789012", "999999999999"),
                    ARN.replace("us-east-1", "eu-west-1"), ARN.replace("arn:aws:", "arn:aws-cn:"),
                    ARN.replace(PREFIX, PREFIX + "extra"), ARN.rsplit(":", 1)[0],
                    ARN + ":4", ARN.replace(":3", ":0"), ARN + "\n"]:
            with self.subTest(arn=arn), self.assertRaises(cleanup.CleanupRefused):
                self.retire(arn=arn)

    def test_unpublished_or_alias_invocation_is_refused(self):
        for suffix in ["", ":$LATEST", ":current", ":0"]:
            context = SimpleNamespace(invoked_function_arn=CONTEXT.invoked_function_arn.rsplit(":", 1)[0] + suffix)
            with self.subTest(suffix=suffix), self.assertRaises(cleanup.CleanupRefused):
                self.retire(context=context)

    def test_native_ownership_mismatch_never_mutates(self):
        for key in TAGS:
            for replacement in [None, "different"]:
                tags = dict(TAGS)
                if replacement is None:
                    del tags[key]
                else:
                    tags[key] = replacement
                self.tags(tags)
                with self.subTest(key=key, replacement=replacement), self.assertRaises(cleanup.CleanupRefused):
                    self.retire()
                self.stub.assert_no_pending_responses()

    def test_duplicate_native_tags_never_mutate(self):
        tags = [{"key": key, "value": value} for key, value in TAGS.items()]
        self.stub.add_response("list_tags_for_resource", {"tags": tags + tags[:1]}, {"resourceArn": ARN})
        with self.assertRaises(cleanup.CleanupRefused):
            self.retire()
        self.stub.assert_no_pending_responses()

    def test_request_cannot_override_installed_boundary_or_operation(self):
        valid = {"action": "delete", "taskDefinitionArn": ARN}
        for event in [None, [], {**valid, "stackPrefix": "another"}, {**valid, "credentials": {}},
                      {**valid, "action": "describe"}, {"action": "delete"}]:
            with self.subTest(event=event), self.assertRaises(cleanup.CleanupRefused):
                cleanup.retire(event, CONTEXT, PREFIX, self.ecs)

    def test_wrong_returned_identity_or_status_is_not_success(self):
        for definition in [{"taskDefinitionArn": ARN + "4", "status": "DELETE_IN_PROGRESS"},
                           {"taskDefinitionArn": ARN, "status": "ACTIVE"}]:
            self.tags()
            self.stub.add_response("delete_task_definitions", {"taskDefinitions": [definition]},
                                   {"taskDefinitions": [ARN]})
            with self.assertRaises(cleanup.CleanupRefused):
                self.retire()
            self.stub.assert_no_pending_responses()

    def test_provider_errors_are_sanitized_and_not_absence(self):
        self.tags()
        self.stub.add_client_error("delete_task_definitions", service_error_code="AccessDeniedException",
                                   service_message="never-return-this", expected_params={"taskDefinitions": [ARN]})
        with patch.object(cleanup.boto3, "client", return_value=self.ecs), patch.dict(os.environ, {"ALIEN_STACK_PREFIX": PREFIX}):
            response = cleanup.handler({"action": "delete", "taskDefinitionArn": ARN}, CONTEXT)
        self.assertEqual(response, {"status": "REFUSED"})
        self.assertNotIn("never-return-this", json.dumps(response))
        self.stub.assert_no_pending_responses()

    def test_native_tag_denial_never_reaches_mutation(self):
        self.stub.add_client_error("list_tags_for_resource", service_error_code="AccessDeniedException",
                                   expected_params={"resourceArn": ARN})
        with patch.object(cleanup.boto3, "client", return_value=self.ecs), patch.dict(os.environ, {"ALIEN_STACK_PREFIX": PREFIX}):
            self.assertEqual(cleanup.handler({"action": "delete", "taskDefinitionArn": ARN}, CONTEXT),
                             {"status": "REFUSED"})
        self.stub.assert_no_pending_responses()

    def test_batch_failure_is_not_accepted_or_disclosed(self):
        self.tags()
        self.stub.add_response("delete_task_definitions", {"taskDefinitions": [], "failures": [
            {"arn": ARN, "reason": "denied", "detail": "never-return-this"}
        ]}, {"taskDefinitions": [ARN]})
        with patch.object(cleanup.boto3, "client", return_value=self.ecs), patch.dict(os.environ, {"ALIEN_STACK_PREFIX": PREFIX}):
            self.assertEqual(cleanup.handler({"action": "delete", "taskDefinitionArn": ARN}, CONTEXT),
                             {"status": "REFUSED"})
        self.stub.assert_no_pending_responses()


if __name__ == "__main__":
    unittest.main()
