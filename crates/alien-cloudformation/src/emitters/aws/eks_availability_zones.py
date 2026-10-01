"""Setup-only CloudFormation lookup for account-local EKS Availability Zones."""
import json
import urllib.request

import boto3
from botocore.config import Config


def handler(event, context):
    physical_id = event.get("PhysicalResourceId") or (
        event["StackId"] + ":" + event["LogicalResourceId"]
    )
    status, reason, data = "SUCCESS", "Availability Zones selected", {}
    try:
        if event["RequestType"] != "Delete":
            properties = event["ResourceProperties"]
            count = int(properties["RequestedCount"])
            if count < 2 or count > 3:
                raise ValueError("EKS requires two or three distinct Availability Zones")
            excluded = set(properties["ExcludedZoneIds"])
            response = boto3.client("ec2", config=Config(
                connect_timeout=5, read_timeout=10,
                retries={"mode": "standard", "total_max_attempts": 1},
            )).describe_availability_zones(
                Filters=[
                    {"Name": "state", "Values": ["available"]},
                    {"Name": "zone-type", "Values": ["availability-zone"]},
                ],
                AllAvailabilityZones=False,
            )
            zones = {}
            for zone in response["AvailabilityZones"]:
                if (zone.get("State") == "available"
                        and zone.get("ZoneType") == "availability-zone"
                        and zone["ZoneId"] not in excluded):
                    zones[zone["ZoneId"]] = zone["ZoneName"]
            selected = sorted(zones)[:count]
            if len(selected) < count or len({zones[z] for z in selected}) != count:
                raise ValueError("Not enough distinct supported EKS Availability Zones")
            data = {"ZoneIds": ",".join(selected), "ZoneNames": ",".join(zones[z] for z in selected)}
    except Exception as error:
        status, reason = "FAILED", str(error)[:1000]
    finally:
        body = json.dumps({
            "Status": status, "Reason": reason, "PhysicalResourceId": physical_id,
            "StackId": event["StackId"], "RequestId": event["RequestId"],
            "LogicalResourceId": event["LogicalResourceId"], "NoEcho": False, "Data": data,
        }).encode("utf-8")
        request = urllib.request.Request(
            event["ResponseURL"], data=body, method="PUT",
            headers={"Content-Type": "", "Content-Length": str(len(body))},
        )
        # Response transport failures must propagate, otherwise CloudFormation waits for timeout.
        with urllib.request.urlopen(request, timeout=20):
            pass
