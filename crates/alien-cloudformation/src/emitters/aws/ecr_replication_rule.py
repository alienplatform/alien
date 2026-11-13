"""Setup-only CloudFormation resource that adds one ECR replication rule to the account.

ECR keeps one replication configuration per account and region, and writing it replaces
every rule. The native CloudFormation resource would drop rules other tools or other
registries wrote, and would clear the whole configuration when the stack is deleted. This
handler reads the current rules at the moment of the write, keeps every rule that is not
this registry's, and puts the list back with this registry's rule added, updated or removed.
"""
import json
import random
import re
import time
import urllib.request

import boto3
from botocore.config import Config

REGION = re.compile(r"^[a-z]{2}(-[a-z]+)+-[0-9]$")

# ECR limits per account and region: rules in a configuration, destinations in one rule.
MAX_RULES = 25
MAX_DESTINATIONS = 10
# Another writer can change the configuration between our read and our write.
MAX_ATTEMPTS = 5


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


def read_rules(ecr):
    return ecr.describe_registry().get("replicationConfiguration", {}).get("rules") or []


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
            if len(regions) > MAX_DESTINATIONS:
                raise ValueError(
                    "%d regions requested and ECR allows at most %d destinations in one rule"
                    % (len(regions), MAX_DESTINATIONS)
                )
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
        for attempt in range(MAX_ATTEMPTS):
            existing = read_rules(ecr)
            rules = merged_rules(existing, prefixes, properties["RegistryId"], ours)
            if rules == existing:
                break
            if len(rules) > MAX_RULES:
                raise ValueError(
                    "The account already has %d replication rules and ECR allows at most %d "
                    "per region; remove a rule that is not needed or leave the regions empty"
                    % (len(rules) - (1 if ours else 0), MAX_RULES)
                )
            ecr.put_replication_configuration(replicationConfiguration={"rules": rules})
            # Read back: the write replaces every rule, so a writer that raced us could
            # have lost its rule, or ours could have been replaced.
            after = read_rules(ecr)
            kept = [rule for rule in rules if rule != ours]
            if all(rule in after for rule in kept) and (ours is None or ours in after) and (
                ours is not None or not any(
                    is_ours(rule, prefix, properties["RegistryId"])
                    for rule in after for prefix in prefixes
                )
            ):
                break
            if attempt == MAX_ATTEMPTS - 1:
                raise RuntimeError(
                    "Another writer kept changing the ECR replication configuration; "
                    "gave up after %d attempts without confirming the rules" % MAX_ATTEMPTS
                )
            time.sleep(random.uniform(0.2, 1.0) * (attempt + 1))
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
