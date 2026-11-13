"""Setup-only CloudFormation resource that adds one ECR replication rule to the account.

ECR keeps one replication configuration per account and region, and writing it replaces
every rule. The native CloudFormation resource would drop rules other tools or other
registries wrote, and would clear the whole configuration when the stack is deleted. This
handler reads the current rules at the moment of the write, keeps every rule that is not
this registry's, and puts the list back with this registry's rule added, updated or removed.
"""
import json
import re
import urllib.request

import boto3
from botocore.config import Config

REGION = re.compile(r"^[a-z]{2}(-[a-z]+)+-[0-9]$")


def is_ours(rule, prefix, registry_id):
    """Exactly the rule this handler writes: this prefix only, destinations in this account.
    A rule on the same prefix that copies to another account is someone else's and is kept."""
    filters = rule.get("repositoryFilters") or []
    destinations = rule.get("destinations") or []
    return (
        len(filters) == 1
        and filters[0].get("filterType") == "PREFIX_MATCH"
        and filters[0].get("filter") == prefix
        and len(destinations) > 0
        and all(d.get("registryId") == registry_id for d in destinations)
    )


def merged_rules(existing, prefixes, registry_id, ours):
    """Every rule that is not one of ours, unchanged and in order, then ours (if any)."""
    kept = [
        rule for rule in existing
        if not any(is_ours(rule, prefix, registry_id) for prefix in prefixes)
    ]
    return kept + ([ours] if ours else [])


def handler(event, context):
    physical_id = event.get("PhysicalResourceId") or (
        event["StackId"] + ":" + event["LogicalResourceId"]
    )
    status, reason = "SUCCESS", "Replication rules updated"
    try:
        properties = event.get("ResourceProperties") or {}
        prefixes = {properties["Prefix"]}
        old = event.get("OldResourceProperties") or {}
        if old.get("Prefix"):
            prefixes.add(old["Prefix"])
        ours = None
        if event["RequestType"] != "Delete":
            regions = []
            for region in properties.get("Regions") or []:
                if not REGION.match(region):
                    raise ValueError("'%s' is not a supported AWS region name" % region)
                if region not in regions:
                    regions.append(region)
            if regions:
                ours = {
                    "destinations": [
                        {"region": region, "registryId": properties["RegistryId"]}
                        for region in regions
                    ],
                    "repositoryFilters": [
                        {"filter": properties["Prefix"], "filterType": "PREFIX_MATCH"}
                    ],
                }
        ecr = boto3.client("ecr", config=Config(
            connect_timeout=5, read_timeout=10,
            retries={"mode": "standard", "total_max_attempts": 3},
        ))
        existing = (
            ecr.describe_registry().get("replicationConfiguration", {}).get("rules") or []
        )
        rules = merged_rules(existing, prefixes, properties["RegistryId"], ours)
        if rules != existing:
            ecr.put_replication_configuration(replicationConfiguration={"rules": rules})
    except Exception as error:
        status, reason = "FAILED", str(error)[:1000]
    finally:
        body = json.dumps({
            "Status": status, "Reason": reason, "PhysicalResourceId": physical_id,
            "StackId": event["StackId"], "RequestId": event["RequestId"],
            "LogicalResourceId": event["LogicalResourceId"], "NoEcho": False, "Data": {},
        }).encode("utf-8")
        request = urllib.request.Request(
            event["ResponseURL"], data=body, method="PUT",
            headers={"Content-Type": "", "Content-Length": str(len(body))},
        )
        # Response transport failures must propagate, otherwise CloudFormation waits for timeout.
        with urllib.request.urlopen(request, timeout=20):
            pass
