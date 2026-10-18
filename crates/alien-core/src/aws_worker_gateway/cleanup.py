"""Setup-owned capability for retiring this deployment's gateway task revisions.

The caller must drain services/tasks first and independently observe final absence.
Only a published, setup-bound Lambda version may be invoked. ECS responses contain
task configuration, so neither provider responses nor exceptions leave this handler.
"""
import os
import re
import uuid

import boto3
from botocore.config import Config


class CleanupRefused(Exception):
    """The request or native identity is outside the installed capability."""


def retire(event, context, stack_prefix, ecs):
    if not isinstance(event, dict) or set(event) != {"action", "taskDefinitionArn"}:
        raise CleanupRefused()
    action, arn = event["action"], event["taskDefinitionArn"]
    if action not in ("deregister", "delete") or not isinstance(arn, str):
        raise CleanupRefused()
    invocation = context.invoked_function_arn.split(":")
    if (len(invocation) != 8 or invocation[0] != "arn" or invocation[2] != "lambda"
            or invocation[5] != "function" or not re.fullmatch(r"[1-9][0-9]*", invocation[7])
            or not re.fullmatch(r"[0-9]{12}", invocation[4])
            or not isinstance(stack_prefix, str)
            or not re.fullmatch(r"[A-Za-z0-9_-]+", stack_prefix)):
        raise CleanupRefused()
    prefix = f"arn:{invocation[1]}:ecs:{invocation[3]}:{invocation[4]}:task-definition/"
    if not arn.startswith(prefix):
        raise CleanupRefused()
    match = re.fullmatch(r"([A-Za-z0-9_-]{1,255}):([1-9][0-9]*)", arn[len(prefix):])
    if not match or not match[1].startswith(stack_prefix + "-"):
        raise CleanupRefused()

    # Read only tags, never DescribeTaskDefinition. A family prefix alone does
    # not authorize deleting a task registered by a different owner.
    native = ecs.list_tags_for_resource(resourceArn=arn)
    tags = {}
    for tag in native["tags"]:
        if tag["key"] in tags:
            raise CleanupRefused()
        tags[tag["key"]] = tag["value"]
    resource, attempt = tags.get("resource"), tags.get("CreateAttempt")
    if (tags.get("deployment") != stack_prefix or tags.get("managed-by") != "runtime"
            or not isinstance(resource, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", resource)
            or not isinstance(attempt, str)):
        raise CleanupRefused()
    try:
        token = uuid.UUID(attempt)
    except ValueError:
        raise CleanupRefused() from None
    if (token.version != 4 or str(token) != attempt
            or match[1] != f"{stack_prefix}-{resource}-gateway-{attempt}"):
        raise CleanupRefused()

    # One durable mutation per invocation. Never convert an error or an accepted
    # asynchronous delete into proof that the revision is absent.
    if action == "deregister":
        response = ecs.deregister_task_definition(taskDefinition=arn)
        definitions = [response["taskDefinition"]]
        expected_status = "INACTIVE"
    else:
        response = ecs.delete_task_definitions(taskDefinitions=[arn])
        if response.get("failures"):
            raise CleanupRefused()
        definitions = response["taskDefinitions"]
        expected_status = "DELETE_IN_PROGRESS"
    if (len(definitions) != 1 or definitions[0].get("taskDefinitionArn") != arn
            or definitions[0].get("status") != expected_status):
        raise CleanupRefused()
    return {"taskDefinitionArn": arn, "status": expected_status}


def handler(event, context):
    try:
        ecs = boto3.client("ecs", config=Config(
            connect_timeout=5, read_timeout=15,
            retries={"mode": "standard", "total_max_attempts": 1},
        ))
        return retire(event, context, os.environ["ALIEN_STACK_PREFIX"], ecs)
    except Exception:
        # Provider errors and malformed responses can include task contents.
        # The runtime treats every refusal as a failed step and retains receipts.
        return {"status": "REFUSED"}
