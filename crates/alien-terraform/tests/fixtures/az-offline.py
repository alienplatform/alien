#!/usr/bin/env python3
"""Synthetic CLI identity for native AzureRM offline planning; no cloud operations."""
import base64
import json
import sys

subscription = "00000000-0000-0000-0000-000000000001"
tenant = "00000000-0000-0000-0000-000000000002"
principal = "00000000-0000-0000-0000-000000000003"
account = {"id": subscription, "tenantId": tenant,
           "user": {"name": principal, "type": "servicePrincipal"}}
args = sys.argv[1:]
if args[0] == "version":
    result = {"azure-cli": "2.80.0"}
elif args[:2] == ["account", "show"]:
    result = account
elif args[:2] == ["account", "list"]:
    result = [account]
elif args[:2] == ["account", "get-access-token"]:
    def encode(value):
        return base64.urlsafe_b64encode(json.dumps(value).encode()).decode().rstrip("=")
    token = encode({"alg": "none"}) + "." + encode({
        "oid": principal, "appid": principal, "tid": tenant, "exp": 4102444800
    }) + ".offline"
    result = {"accessToken": token, "expiresOn": "2100-01-01 00:00:00.000000",
              "tokenType": "Bearer", "tenant": tenant}
else:
    raise SystemExit("Unsupported offline Azure CLI call: " + repr(args))
print(json.dumps(result))
