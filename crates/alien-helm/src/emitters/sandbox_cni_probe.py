"""Helm install gate: paired live traffic proves that ingress and egress NetworkPolicy work."""

import json
import os
import pathlib
import ssl
import time
import urllib.error
import urllib.request
import uuid


def main():
    namespace = os.environ["PROBE_NAMESPACE"]
    job = os.environ["PROBE_JOB"]
    image = os.environ["PROBE_IMAGE"]
    credentials = pathlib.Path("/var/run/secrets/kubernetes.io/serviceaccount")
    context = ssl.create_default_context(cafile=str(credentials / "ca.crt"))
    # Bypass proxy environment: this is the authenticated in-cluster API endpoint.
    api = urllib.request.build_opener(
        urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=context)
    )
    host = os.environ["KUBERNETES_SERVICE_HOST"]
    host = f"[{host}]" if ":" in host else host
    origin = f"https://{host}:{os.environ['KUBERNETES_SERVICE_PORT']}"

    def request(method, path, body=None):
        token = (credentials / "token").read_text().strip()
        req = urllib.request.Request(
            origin + path,
            data=json.dumps(body).encode() if body is not None else None,
            method=method,
            headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"},
        )
        with api.open(req, timeout=10) as response:
            return json.load(response)

    pods = f"/api/v1/namespaces/{namespace}/pods"
    policies = f"/apis/networking.k8s.io/v1/namespaces/{namespace}/networkpolicies"
    owner = request("GET", f"/apis/batch/v1/namespaces/{namespace}/jobs/{job}")
    owner_ref = [{"apiVersion": "batch/v1", "kind": "Job", "name": job,
                  "uid": owner["metadata"]["uid"]}]
    prefix = "alien-cni-" + uuid.uuid4().hex[:12]
    created = []
    deadline = time.monotonic() + 210
    marker = uuid.uuid4().hex
    server = (
        "from http.server import BaseHTTPRequestHandler,HTTPServer\n"
        "class Handler(BaseHTTPRequestHandler):\n"
        " def do_GET(self):\n"
        "  self.send_response(200); self.end_headers(); "
        f"self.wfile.write({marker.encode()!r})\n"
        " def log_message(self,*args): pass\n"
        "HTTPServer(('0.0.0.0',8080),Handler).serve_forever()\n"
    )

    def create(path, resource):
        resource["metadata"]["ownerReferences"] = owner_ref
        result = request("POST", path, resource)
        created.append(path + "/" + result["metadata"]["name"])
        return result

    def pod(role, code):
        return {
            "apiVersion": "v1", "kind": "Pod",
            "metadata": {"name": prefix + "-" + role,
                         "labels": {"alien.dev/cni-probe": prefix, "alien.dev/cni-role": role}},
            "spec": {
                "automountServiceAccountToken": False, "restartPolicy": "Never",
                "activeDeadlineSeconds": 240, "terminationGracePeriodSeconds": 1,
                "securityContext": {"runAsNonRoot": True, "runAsUser": 65534,
                                    "seccompProfile": {"type": "RuntimeDefault"}},
                "containers": [{"name": "probe", "image": image,
                    "command": ["python3", "-B", "-c", code],
                    "terminationMessagePolicy": "FallbackToLogsOnError",
                    "securityContext": {"allowPrivilegeEscalation": False,
                                        "readOnlyRootFilesystem": True,
                                        "capabilities": {"drop": ["ALL"]}},
                    "resources": {"requests": {"cpu": "10m", "memory": "24Mi"},
                                  "limits": {"cpu": "100m", "memory": "64Mi"}}}],
            },
        }

    def wait(role, predicate):
        while time.monotonic() < deadline:
            current = request("GET", pods + "/" + prefix + "-" + role)
            phase = current.get("status", {}).get("phase")
            if phase == "Failed":
                details = [status.get("state", {}).get("terminated", {}).get("message", "")
                           for status in current.get("status", {}).get("containerStatuses", [])]
                raise RuntimeError(f"{role} probe failed: NetworkPolicy enforcement is unproven; "
                                   + " ".join(details)[:2048])
            if predicate(current):
                print(f"{role}: {phase}; node={current['spec'].get('nodeName')}; "
                      f"ip={current.get('status', {}).get('podIP')}", flush=True)
                return current
            time.sleep(1)
        raise RuntimeError(f"{role} probe deadline: refusing unproven NetworkPolicy enforcement")

    try:
        # The server and positive control have no policy. Only the subject is denied.
        create(policies, {
            "apiVersion": "networking.k8s.io/v1", "kind": "NetworkPolicy",
            "metadata": {"name": prefix},
            "spec": {"podSelector": {"matchLabels": {
                "alien.dev/cni-probe": prefix, "alien.dev/cni-role": "denied"}},
                "policyTypes": ["Egress"], "egress": []},
        })
        listener = pod("listener", server)
        listener["spec"]["containers"][0]["readinessProbe"] = {
            "httpGet": {"path": "/", "port": 8080}, "periodSeconds": 1}
        create(pods, listener)
        listener = wait("listener", lambda p: any(
            c["type"] == "Ready" and c["status"] == "True"
            for c in p.get("status", {}).get("conditions", [])))
        ip = listener["status"]["podIP"]
        authority = f"[{ip}]" if ":" in ip else ip
        url = f"http://{authority}:8080/"
        probe = (
            "import errno,socket,time,urllib.request,urllib.error\n"
            "def require(condition,message):\n"
            " if not condition: raise RuntimeError(message)\n"
            "opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))\n"
            "for attempt in range(3):\n"
            " try:\n"
            f"  body=opener.open({url!r},timeout=3).read()\n"
            f"  require(body=={marker.encode()!r}, 'wrong positive control body')\n"
            "  require(not DENIED, 'NetworkPolicy did not block traffic')\n"
            " except (urllib.error.URLError,TimeoutError,OSError) as error:\n"
            "  reason=getattr(error,'reason',error)\n"
            "  require(DENIED and (isinstance(reason,TimeoutError) or "
            "getattr(reason,'errno',None) in (errno.ECONNREFUSED,errno.ENETUNREACH,"
            "errno.EHOSTUNREACH,errno.ETIMEDOUT)), repr(error))\n"
            " time.sleep(1)\n"
        )
        # A successful negative result is only meaningful bracketed by working controls.
        for role, denied in [("control", False), ("denied", True), ("control-after", False)]:
            create(pods, pod(role, f"DENIED={denied!r}\n" + probe))
            wait(role, lambda p: p.get("status", {}).get("phase") == "Succeeded")
        # An egress-only implementation must not pass the ingress endpoint gate.
        ingress_name = prefix + "-ingress"
        create(policies, {
            "apiVersion": "networking.k8s.io/v1", "kind": "NetworkPolicy",
            "metadata": {"name": ingress_name},
            "spec": {"podSelector": {"matchLabels": {
                "alien.dev/cni-probe": prefix, "alien.dev/cni-role": "listener"}},
                "policyTypes": ["Ingress"], "ingress": []},
        })
        create(pods, pod("ingress-denied", "DENIED=True\n" + probe))
        wait("ingress-denied", lambda p: p.get("status", {}).get("phase") == "Succeeded")
        request("DELETE", policies + "/" + ingress_name, {"gracePeriodSeconds": 0})
        time.sleep(2)
        create(pods, pod("ingress-control-after", "DENIED=False\n" + probe))
        wait("ingress-control-after", lambda p: p.get("status", {}).get("phase") == "Succeeded")
        print("NetworkPolicy enforced: ingress and egress denied requests; all controls passed")
    finally:
        failures = []
        for path in reversed(created):
            try:
                request("DELETE", path, {"gracePeriodSeconds": 0})
            except urllib.error.HTTPError as error:
                if error.code != 404:
                    failures.append(path)
        if failures:
            raise RuntimeError("Could not clean up CNI probe objects: " + ", ".join(failures))


if __name__ == "__main__":
    main()
