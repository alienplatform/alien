import json
import os
import ssl
import sys
import urllib.error
import urllib.request

NAMESPACE = os.environ["SANDBOX_NAMESPACE"]
RUNTIME_CLASS = os.environ["SANDBOX_RUNTIME_CLASS"]
IMAGE = os.environ["PROBE_IMAGE"]
CAPABILITIES = os.environ["BOOT_CAPABILITIES"].split(",")
SERVICE_ACCOUNT = "/var/run/secrets/kubernetes.io/serviceaccount"

FIX = (
    "Root-boot sandboxes need a namespace that admits Pod Security 'baseline'. Create and label "
    "it, then set sandbox.namespace=%s:\n"
    "  kubectl create namespace %s && kubectl label namespace %s "
    "pod-security.kubernetes.io/enforce=baseline"
) % (NAMESPACE, NAMESPACE, NAMESPACE)


def fail(message):
    print("sandbox.namespace check failed: " + message, file=sys.stderr, flush=True)
    sys.exit(1)


def container(name, capabilities, run_as_root=True):
    return {
        "name": name,
        "image": IMAGE,
        "securityContext": {
            "allowPrivilegeEscalation": False,
            "privileged": False,
            "runAsNonRoot": not run_as_root,
            "capabilities": {"drop": ["ALL"], **({"add": capabilities} if capabilities else {})},
        },
    }


# The shape of a root-boot sandbox pod: root, the bounded boot capability set, the staging init
# container that holds none, and the read-only subPath mounts over the agent and /opt/alien.
pod = {
    "apiVersion": "v1",
    "kind": "Pod",
    "metadata": {"name": "alien-root-boot-check", "namespace": NAMESPACE},
    "spec": {
        "runtimeClassName": RUNTIME_CLASS or None,
        "automountServiceAccountToken": False,
        "restartPolicy": "Never",
        "securityContext": {
            "runAsUser": 0,
            "runAsNonRoot": False,
            "seccompProfile": {"type": "RuntimeDefault"},
        },
        "volumes": [{"name": "protected", "emptyDir": {}}],
        "initContainers": [
            {
                **container("stage", None),
                "volumeMounts": [{"name": "protected", "mountPath": "/stage"}],
            }
        ],
        "containers": [
            {
                **container("sandbox", CAPABILITIES),
                "volumeMounts": [
                    {"name": "protected", "mountPath": "/opt/alien", "subPath": "opt", "readOnly": True}
                ],
            }
        ],
    },
}
if not RUNTIME_CLASS:
    del pod["spec"]["runtimeClassName"]

host = os.environ["KUBERNETES_SERVICE_HOST"]
port = os.environ.get("KUBERNETES_SERVICE_PORT", "443")
host = "[%s]" % host if ":" in host else host
token = open(SERVICE_ACCOUNT + "/token").read().strip()
context = ssl.create_default_context(cafile=SERVICE_ACCOUNT + "/ca.crt")
request = urllib.request.Request(
    "https://%s:%s/api/v1/namespaces/%s/pods?dryRun=All&fieldManager=alien-root-boot-check"
    % (host, port, NAMESPACE),
    data=json.dumps(pod).encode(),
    headers={"Authorization": "Bearer " + token, "Content-Type": "application/json"},
    method="POST",
)
try:
    with urllib.request.urlopen(request, context=context, timeout=30) as response:
        print("a root-boot pod is admitted in namespace %s (dry run, status %s)" % (NAMESPACE, response.status), flush=True)
        sys.exit(0)
except urllib.error.HTTPError as error:
    body = error.read().decode("utf-8", "replace")
    try:
        reason = json.loads(body).get("message", body)
    except ValueError:
        reason = body
    if error.code == 404:
        fail("namespace %s does not exist. %s" % (NAMESPACE, FIX))
    if error.code == 403 and "violates PodSecurity" in reason:
        fail("namespace %s refuses root-boot pods (%s). %s" % (NAMESPACE, reason, FIX))
    if error.code == 403 and "RuntimeClass" in reason:
        fail("the sandbox RuntimeClass is not usable in namespace %s: %s" % (NAMESPACE, reason))
    fail("namespace %s refused the dry-run pod (HTTP %s): %s" % (NAMESPACE, error.code, reason))
except OSError as error:
    fail("cannot reach the API server: %s" % error)
