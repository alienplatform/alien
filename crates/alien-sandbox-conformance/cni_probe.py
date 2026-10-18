#!/usr/bin/env python3
"""Measure NetworkPolicy enforcement in a fresh, isolated namespace."""

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kubeconfig", required=True)
    parser.add_argument("--image", default="python:3.13-slim")
    parser.add_argument("--namespace-prefix", default="alien-cni-probe-")
    args = parser.parse_args()
    namespace = args.namespace_prefix + uuid.uuid4().hex[:12]

    def kubectl(*arguments, document=None):
        result = subprocess.run(
            ["kubectl", "--kubeconfig", args.kubeconfig, *arguments],
            input=json.dumps(document) if document is not None else None,
            text=True,
            capture_output=True,
            timeout=90,
            check=True,
        )
        return result.stdout

    # Create separately: a failed create must never delete somebody else's namespace.
    kubectl("create", "namespace", namespace)
    try:
        body = b"alien-network-policy-control\n"
        server = (
            "from http.server import BaseHTTPRequestHandler, HTTPServer\n"
            "class Handler(BaseHTTPRequestHandler):\n"
            " def do_GET(self):\n"
            "  self.send_response(200); self.end_headers()\n"
            f"  self.wfile.write({body!r})\n"
            " def log_message(self, *args): pass\n"
            "HTTPServer(('0.0.0.0',8080),Handler).serve_forever()\n"
        )
        resources = []
        for role in ["listener", "control", "denied"]:
            container = {
                "name": "probe",
                "image": args.image,
                "command": ["python3", "-c", server]
                if role == "listener"
                else ["sleep", "240"],
                "securityContext": {
                    "allowPrivilegeEscalation": False,
                    "capabilities": {"drop": ["ALL"]},
                    "runAsNonRoot": True,
                    "runAsUser": 1000,
                    "seccompProfile": {"type": "RuntimeDefault"},
                },
                "resources": {
                    "requests": {"cpu": "10m", "memory": "32Mi"},
                    "limits": {"cpu": "100m", "memory": "64Mi"},
                },
            }
            if role == "listener":
                container["readinessProbe"] = {
                    "httpGet": {"path": "/", "port": 8080},
                    "periodSeconds": 1,
                }
            resources.append({
                "apiVersion": "v1",
                "kind": "Pod",
                "metadata": {"name": role, "labels": {"role": role}},
                "spec": {
                    "automountServiceAccountToken": False,
                    "restartPolicy": "Never",
                    "terminationGracePeriodSeconds": 1,
                    "activeDeadlineSeconds": 240,
                    "containers": [container],
                },
            })
        resources.append({
            "apiVersion": "networking.k8s.io/v1",
            "kind": "NetworkPolicy",
            "metadata": {"name": "deny-egress"},
            "spec": {
                "podSelector": {"matchLabels": {"role": "denied"}},
                "policyTypes": ["Egress"],
                "egress": [],
            },
        })
        kubectl("-n", namespace, "create", "-f", "-", document={
            "apiVersion": "v1", "kind": "List", "items": resources,
        })
        kubectl("-n", namespace, "wait", "--for=condition=Ready", "pod",
                "--all", "--timeout=60s")
        pods = json.loads(kubectl("-n", namespace, "get", "pods", "-o", "json"))
        placements = {
            pod["metadata"]["name"]: {
                "node": pod["spec"]["nodeName"], "ip": pod["status"]["podIP"],
            }
            for pod in pods["items"]
        }
        ip = placements["listener"]["ip"]
        host = f"[{ip}]" if ":" in ip else ip
        request = json.dumps({
            "kind": "http", "url": f"http://{host}:8080/", "useProxy": False,
        })
        probe = (pathlib.Path(__file__).parent / "src" / "probe.py").read_text()
        attempts = []
        for index in range(3):
            if index:
                time.sleep(2)
            pair = {}
            for role in ["control", "denied"]:
                pair[role] = json.loads(kubectl(
                    "-n", namespace, "exec", role, "--", "python3", "-c", probe, request,
                ))
            attempts.append(pair)
        controls_work = all(
            pair["control"].get("status") == 200
            and pair["control"].get("bodySha256") == hashlib.sha256(body).hexdigest()
            for pair in attempts
        )
        blocked = all(
            pair["denied"]["outcome"] in ["timeout", "refused", "unreachable"]
            for pair in attempts
        )
        verdict = "unproven" if not controls_work else "pass" if blocked else "fail"
        report = {"namespace": namespace, "placements": placements,
                  "attempts": attempts, "verdict": verdict}
    finally:
        kubectl("delete", "namespace", namespace, "--wait=true", "--timeout=60s")
    report["namespaceDeleted"] = True
    print(json.dumps(report, indent=2))
    return 0 if verdict == "pass" else 1


if __name__ == "__main__":
    sys.exit(main())
