"""Run a setup reapply and require the VPC's NAT allocations and addresses to survive."""
import argparse
import json
import pathlib
import subprocess
import sys


def nat_state(vpc_id):
    response = json.loads(subprocess.check_output([
        "aws", "ec2", "describe-nat-gateways", "--filter",
        f"Name=vpc-id,Values={vpc_id}", "Name=state,Values=available",
        "--output", "json",
    ]))
    gateways = response["NatGateways"]
    if not gateways:
        raise RuntimeError("the test VPC has no available NAT gateway")
    allocations = sorted([
        {"gatewayId": gateway["NatGatewayId"],
         "allocationId": address["AllocationId"], "publicIp": address["PublicIp"]}
        for gateway in gateways for address in gateway["NatGatewayAddresses"]
        if address.get("PublicIp") and address.get("AllocationId")
    ], key=lambda address: (address["gatewayId"], address["allocationId"]))
    if not allocations:
        raise RuntimeError("the test VPC has no public NAT allocation")
    return {"allocations": allocations,
            "egressPublicIps": sorted({address["publicIp"] for address in allocations})}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vpc-id", required=True)
    parser.add_argument("--report", type=pathlib.Path, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER,
                        help="setup apply command after --; must wait for completion")
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("an apply command is required")
    # Exclusive creation keeps a rerun from replacing prior evidence.
    with args.report.open("x") as output:
        report = {"vpcId": args.vpc_id, "passed": False}
        try:
            report["before"] = nat_state(args.vpc_id)
            report["applyExitCode"] = subprocess.run(command, check=False).returncode
            report["after"] = nat_state(args.vpc_id)
            report["passed"] = (report["applyExitCode"] == 0
                                and report["before"] == report["after"])
        finally:
            json.dump(report, output, indent=2)
            output.write("\n")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
