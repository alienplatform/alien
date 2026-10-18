"""Read the effective subnet routes before allowing sandbox setup to continue."""

import json
import urllib.request

import boto3
from botocore.config import Config


def validate(ec2, properties):
    vpc = properties["VpcId"]
    sandbox_ids = properties["SandboxSubnetIds"]
    gateway_ids = properties.get("GatewaySubnetIds", [])
    workload_ids = properties.get("WorkloadSubnetIds", [])
    if not sandbox_ids or set(sandbox_ids) & (set(gateway_ids) | set(workload_ids)):
        raise ValueError("Sandbox connector subnets must be dedicated and nonempty")
    if set(gateway_ids) & set(workload_ids):
        raise ValueError("Gateway subnets must be dedicated")
    vpc_data = ec2.describe_vpcs(VpcIds=[vpc])["Vpcs"][0]
    ids = sorted(set(sandbox_ids + gateway_ids + workload_ids))
    subnets = {
        subnet["SubnetId"]: subnet
        for subnet in ec2.describe_subnets(SubnetIds=ids)["Subnets"]
    }
    if set(subnets) != set(ids) or any(s["VpcId"] != vpc for s in subnets.values()):
        raise ValueError("Every subnet must belong to the deployment VPC")
    for selected in (sandbox_ids, gateway_ids):
        if selected and len({subnets[s]["AvailabilityZoneId"] for s in selected}) < 2:
            raise ValueError("Sandbox and gateway subnets must each span at least two availability zones")
    tables = [
        table
        for page in ec2.get_paginator("describe_route_tables").paginate(
            Filters=[{"Name": "vpc-id", "Values": [vpc]}]
        )
        for table in page["RouteTables"]
    ]
    main = [t for t in tables if any(a.get("Main") for a in t["Associations"])]
    if len(main) != 1:
        raise ValueError("Cannot determine the VPC main route table")

    def routes(subnet_id):
        explicit = [
            t for t in tables
            if any(a.get("SubnetId") == subnet_id for a in t["Associations"])
        ]
        if len(explicit) > 1:
            raise ValueError("Subnet has ambiguous route-table associations")
        return (explicit or main)[0]["Routes"]

    for subnet_id in sandbox_ids:
        if subnets[subnet_id].get("Ipv6CidrBlockAssociationSet"):
            raise ValueError("Sandbox connector subnets must not have IPv6 CIDRs")
        if any(r.get("GatewayId") != "local" for r in routes(subnet_id)):
            raise ValueError("Sandbox connector subnets must have only local VPC routes")
    nat_ids = set()
    for subnet_id in gateway_ids + (workload_ids if gateway_ids else []):
        selected_routes = routes(subnet_id)
        defaults = [r for r in selected_routes if r.get("DestinationCidrBlock") == "0.0.0.0/0"]
        if len(defaults) != 1 or not defaults[0].get("NatGatewayId") or defaults[0].get("State") != "active":
            raise ValueError("Gateway and workload subnets must use an active NAT gateway default route")
        if subnet_id in gateway_ids and any(r.get("GatewayId") != "local" and r != defaults[0] for r in selected_routes):
            raise ValueError("Gateway subnets must have only local VPC routes and the NAT default route")
        nat_ids.add(defaults[0]["NatGatewayId"])
    if len(nat_ids) > 1:
        raise ValueError("Gateway and workloads must share the deployment NAT gateway")
    if gateway_ids and len(gateway_ids) not in (2, 3):
        raise ValueError("Provide two or three dedicated gateway subnets")
    data = {
        "SandboxSubnetIds": sandbox_ids,
        "GatewaySubnetIds": gateway_ids,
        "SandboxCidrs": [subnets[s]["CidrBlock"] for s in sandbox_ids],
        "GatewayCidrs": [subnets[s]["CidrBlock"] for s in gateway_ids],
        "WorkloadCidrs": [subnets[s]["CidrBlock"] for s in workload_ids],
        "VpcCidrs": [c["CidrBlock"] for c in vpc_data["CidrBlockAssociationSet"] if c.get("CidrBlockState", {}).get("State", "associated") == "associated"],
    }
    # ToJsonString rewrites opaque GetAtt values as string substitutions. Return
    # CSV; the importer normalizes it like the existing network subnet fields.
    data = {name: ",".join(values) for name, values in data.items()}
    # A link grants access to every declared gateway port, including authenticated admin.
    data["ConnectorEgress"] = [{"IpProtocol": "tcp", "FromPort": 3128, "ToPort": 3129, "CidrIp": subnets[s]["CidrBlock"]} for s in gateway_ids]
    return data


def handler(event, context):
    status, reason, data = "SUCCESS", "Subnet routes validated", {}
    try:
        if event["RequestType"] != "Delete":
            data = validate(boto3.client("ec2", config=Config(connect_timeout=5, read_timeout=10, retries={"total_max_attempts": 1})), event["ResourceProperties"])
    except Exception as error:
        status, reason = "FAILED", str(error)[:1000]
    response = json.dumps({
        "Status": status,
        "Reason": reason,
        "PhysicalResourceId": event.get("PhysicalResourceId", event["LogicalResourceId"]),
        "StackId": event["StackId"],
        "RequestId": event["RequestId"],
        "LogicalResourceId": event["LogicalResourceId"],
        "Data": data,
    }).encode()
    request = urllib.request.Request(event["ResponseURL"], data=response, method="PUT", headers={"Content-Type": ""})
    with urllib.request.urlopen(request, timeout=15) as response:
        response.read()
