"""Helm install gate: paired live traffic proves that ingress and egress NetworkPolicy work."""

import json
import os
import pathlib
import ssl
import time
import urllib.error
import urllib.request
import uuid


# Where the cluster has a NetworkPolicy controller that is off by default, name the switch.
FIX_HINT = (
    "Turn on NetworkPolicy enforcement and re-run the install. "
    "EKS Auto Mode: kubectl apply a ConfigMap amazon-vpc-cni in kube-system with data "
    "enable-network-policy-controller: \"true\" (the NodeClass networkPolicy field only sets the "
    "default). EKS with the VPC CNI add-on: enable network policy in the add-on configuration. "
    "GKE: enable network policy (Dataplane V2 or Calico). AKS: create the cluster with "
    "--network-policy azure|calico|cilium."
)


# Time budget of one run, in one place. The script's deadline and the Job's activeDeadlineSeconds
# (sandbox_cni.rs reads JOB_DEADLINE_SECONDS from this file) come from these numbers. Helm's
# timeout (`--timeout 15m` in the install instructions, `timeout: 15m` in the Flux HelmRelease)
# applies to each hook, so the Job must fit inside it. A pod start is ~35 s under a VM-per-pod
# runtime (Kata). The listener is the first pod on the sandbox nodes: a node pool that scales
# from zero has to provision a node for it, so it may stay Pending for PENDING_CAUSE_BOUND_SECONDS
# (the wait the install had before this budget). The clients then land on that node. A run that
# passes needs:
#   API_READY_SECONDS      60   the runner's own network is applied (the wait below)
#   ADMISSION_SECONDS      30   both admission policies answer
#   PENDING_CAUSE_BOUND   210   the listener is scheduled and starts (a node may be provisioned)
#   stage 1: start + run   35 + 15   (three clients start together; 3 attempts of <= 4 s)
#   stage 2: start + run   35 + 120  (the recovering control makes up to 30 attempts of <= 4 s)
#   one recreate           45 + 35   (START_BOUND_SECONDS stuck, then a new start)
#   CLEANUP_SECONDS        10   delete what was created
#                         ---
#                         595
# The Job adds the runner's own start (image pull, 30 s) and a 30 s margin: 655 s, inside Helm's
# 15 minutes (900 s) with the next hook's own wait still to come.
API_READY_SECONDS = 60
ADMISSION_SECONDS = 30
POD_START_SECONDS = 35
CLIENT_RUN_SECONDS = 15
RECOVER_RUN_SECONDS = 120
CLEANUP_SECONDS = 10
# How long a pod may stay Pending with a container the runtime failed to create, before it is
# treated as not started and recreated. A retry the kubelet makes by itself succeeds within its
# first backoff steps (10 s, then 20 s), so waiting longer than this is not worth the budget.
# (This is the stuck-start bound; PENDING_CAUSE_BOUND_SECONDS below is a separate one.)
START_BOUND_SECONDS = 45
# How long a pod may stay Pending, unschedulable or failing to pull its image, before the run
# fails naming that cause. The cause does not change by recreating the pod, so there is no retry.
# It is at least the wait the install had before (210 s), so a node pool that scales from zero
# is not failed early.
PENDING_CAUSE_BOUND_SECONDS = 210
SCRIPT_BUDGET_SECONDS = 595
RUNNER_START_SECONDS = 30
BUDGET_MARGIN_SECONDS = 30
JOB_DEADLINE_SECONDS = 655
# Every probe pod's own deadline (counted by the kubelet from the pod's start). The listener must
# outlive every later control, so each pod gets the whole run's budget: the script's own deadline
# ends the run, never a pod's.
POD_DEADLINE_SECONDS = SCRIPT_BUDGET_SECONDS


def client_code(url, marker):
    """Source of the client pods. `DENIED` and `RECOVER` are set by the caller before it."""
    return (
        "import errno,socket,time,urllib.request,urllib.error\n"
        "def require(condition,message):\n"
        " if not condition: raise RuntimeError(message)\n"
        "opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))\n"
        # A recovering control waits, within the pod deadline, for a CNI that removes a
        # deleted policy slowly; it must still be reached once.
        "RECOVER=globals().get('RECOVER',False)\n"
        "reached=False\n"
        "for attempt in range(30 if RECOVER else 3):\n"
        " try:\n"
        f"  body=opener.open({url!r},timeout=3).read()\n"
        f"  require(body=={marker.encode()!r}, 'wrong positive control body')\n"
        "  require(not DENIED, 'NetworkPolicy did not block traffic')\n"
        "  reached=True\n"
        "  if RECOVER: break\n"
        " except (urllib.error.URLError,TimeoutError,OSError) as error:\n"
        "  reason=getattr(error,'reason',error)\n"
        "  require((DENIED or RECOVER) and (isinstance(reason,TimeoutError) or "
        "getattr(reason,'errno',None) in (errno.ECONNREFUSED,errno.ENETUNREACH,"
        "errno.EHOSTUNREACH,errno.ETIMEDOUT)), repr(error))\n"
        " time.sleep(1)\n"
        "require(DENIED or reached, 'traffic did not resume after removing the policy')\n"
    )


# A successful negative result is only meaningful bracketed by working controls. The clients of
# a stage do not depend on each other, so a stage starts them together and then waits for each:
# on a runtime where a pod takes tens of seconds to start (a VM per pod), six one-after-the-other
# starts do not fit in the install's wait. The `ingress-denied` pod may leave its own node; the
# listener's ingress allow list is what refuses it.
STAGES = [
    [("control", False, False), ("denied", True, False), ("ingress-denied", True, False)],
    [("control-after", False, False), ("ingress-control-after", False, True)],
]


def run_stages(start_client, wait_client):
    for stage in STAGES:
        for role, denied, recover in stage:
            start_client(role, denied, recover)
        for role, _, _ in stage:
            wait_client(role)


# A client pod whose probe container never ran says nothing about NetworkPolicy: the sandbox
# runtime could not start it. These are the container states the kubelet reports for that.
RUNTIME_START_REASONS = {
    "StartError", "ContainerCannotRun", "CreateContainerError", "RunContainerError",
    "CreateContainerConfigError",
}


def _terminated_messages(pod):
    return [status.get("state", {}).get("terminated", {}).get("message", "")
            for status in pod.get("status", {}).get("containerStatuses", [])]


def runtime_start_failure(pod, pending_seconds):
    """The runtime's message when the probe container never ran, else None.

    A probe that ran is judged by its own verdict. A container that was never started carries
    the runtime's reason and message instead, either as a failed pod or as a pod stuck waiting
    past START_BOUND_SECONDS.
    """
    for status in pod.get("status", {}).get("containerStatuses", []):
        state = status.get("state", {})
        terminated = state.get("terminated")
        if terminated:
            started = terminated.get("startedAt") or ""
            # The kubelet reports a container the runtime could not start with a start-failure
            # reason and a zero or missing start time. Either alone is not enough: a pod that ran
            # and was lost with its node (ContainerStatusUnknown, exit 137) has no start time
            # either, and it is judged by its failure, not recreated.
            never_started = (terminated.get("reason") in RUNTIME_START_REASONS
                             and (not started or started[:4] in ("0001", "1970")))
            if never_started:
                return terminated.get("message") or terminated.get("reason") or "container not started"
        waiting = state.get("waiting")
        if waiting and waiting.get("reason") in RUNTIME_START_REASONS and pending_seconds > START_BOUND_SECONDS:
            return waiting.get("message") or waiting.get("reason")
    return None


IMAGE_PULL_REASONS = {"ErrImagePull", "ImagePullBackOff", "InvalidImageName", "ErrImageNeverPull"}


def pending_cause(pod, pending_seconds):
    """Why a pod stays Pending (not scheduled, or its image does not pull), else None.

    Only a pod that has been Pending longer than PENDING_CAUSE_BOUND_SECONDS has a cause worth
    naming: a node being provisioned or a first pull attempt are ordinary and short.
    """
    if pod.get("status", {}).get("phase") != "Pending" or pending_seconds <= PENDING_CAUSE_BOUND_SECONDS:
        return None
    for condition in pod.get("status", {}).get("conditions", []):
        if condition.get("type") == "PodScheduled" and condition.get("status") == "False":
            return "the pod was not scheduled (%s): %s" % (
                condition.get("reason") or "Unschedulable", condition.get("message") or "no message")
    for status in pod.get("status", {}).get("containerStatuses", []):
        waiting = status.get("state", {}).get("waiting") or {}
        if waiting.get("reason") in IMAGE_PULL_REASONS:
            return "the pod's image did not pull (%s): %s" % (
                waiting["reason"], waiting.get("message") or "no message")
    return None


def replace_object(request, path, resource, deadline, now=time.monotonic, sleep=time.sleep):
    """Delete an object, wait until it is gone, create it again.

    A pod deleted with no grace period can stay in the API for a moment (Terminating); creating
    the same name then answers 409, so the new object is only created once the old one is 404.
    """
    name = resource["metadata"]["name"]
    try:
        request("DELETE", path + "/" + name, {"gracePeriodSeconds": 0})
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise
    while True:
        try:
            request("GET", path + "/" + name)
        except urllib.error.HTTPError as gone:
            if gone.code == 404:
                break
            raise
        if now() >= deadline:
            raise RuntimeError(f"{name} is still being deleted at the probe's deadline; cannot recreate it")
        sleep(1)
    return request("POST", path, resource)


def wait_for_pod(request, pods, prefix, role, predicate, deadline, recreate, hint,
                 now=time.monotonic, sleep=time.sleep, out=print, created_at=None):
    """Wait for a pod to satisfy `predicate`, recreating it once if the runtime failed to start it.

    `created_at` is the time the pod was created (same clock as `now`). Pods of a stage are
    created together and waited for one by one, so a pod's stuck time counts from its creation,
    not from when this wait began.
    """
    first_seen = now() if created_at is None else created_at
    recreated = False
    current = {}
    while now() < deadline:
        current = request("GET", pods + "/" + prefix + "-" + role)
        phase = current.get("status", {}).get("phase")
        failure = runtime_start_failure(current, now() - first_seen) if phase in ("Failed", "Pending") else None
        if failure:
            if recreated:
                raise RuntimeError(f"runtime failed to start {role} pod: {failure}")
            out(f"runtime failed to start {role} pod: {failure[:512]}; recreated", flush=True)
            recreate(role)
            recreated = True
            first_seen = now()
            sleep(1)
            continue
        cause = pending_cause(current, now() - first_seen)
        if cause:
            raise RuntimeError(f"{role} pod never ran, so NetworkPolicy was not tested: {cause}")
        if phase == "Failed":
            details = _terminated_messages(current)
            raise RuntimeError(f"{role} probe failed: NetworkPolicy enforcement is unproven; "
                               + " ".join(details)[:1024] + " | " + hint)
        if predicate(current):
            notes = _terminated_messages(current)
            if any(notes):
                out(f"{role}: " + " ".join(notes)[:256], flush=True)
            out(f"{role}: {phase}; node={current['spec'].get('nodeName')}; "
                f"ip={current.get('status', {}).get('podIP')}", flush=True)
            return current
        sleep(1)
    cause = pending_cause(current, float("inf"))
    if cause:
        raise RuntimeError(f"{role} pod never ran before the probe's deadline, so NetworkPolicy was not tested: {cause}")
    raise RuntimeError(f"{role} probe deadline: refusing unproven NetworkPolicy enforcement. " + hint)


def main():
    namespace = os.environ["PROBE_NAMESPACE"]
    job = os.environ["PROBE_JOB"]
    image = os.environ["PROBE_IMAGE"]
    pull_secrets = [{"name": n} for n in os.environ.get("PROBE_PULL_SECRETS", "").split(",") if n]
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
    # One deadline for the whole run, counted from here (see the budget above).
    start = time.monotonic()
    deadline = start + SCRIPT_BUDGET_SECONDS
    # A cluster that denies pod traffic by default admits this pod's API calls only once the
    # CNI has applied the runner policy, shortly after the pod starts.
    while True:
        try:
            owner = request("GET", f"/apis/batch/v1/namespaces/{namespace}/jobs/{job}")
            break
        except (urllib.error.URLError, TimeoutError) as error:
            if isinstance(error, urllib.error.HTTPError) or time.monotonic() - start > API_READY_SECONDS:
                raise
            time.sleep(2)
    owner_ref = [{"apiVersion": "batch/v1", "kind": "Job", "name": job,
                  "uid": owner["metadata"]["uid"]}]
    # Fixed names: the probe's Role may read and delete only these objects.
    prefix = job
    runtime_class = os.environ["PROBE_RUNTIME_CLASS"]
    # Where sessions run (`sandbox.nodeSelector`, `sandbox.tolerations`): the probe's clients must
    # be tested on those nodes, not on whichever node the scheduler prefers.
    node_selector = json.loads(os.environ.get("PROBE_NODE_SELECTOR") or "{}")
    tolerations = json.loads(os.environ.get("PROBE_TOLERATIONS") or "[]")
    created = []
    specs = {}
    admission_deadline = time.monotonic() + ADMISSION_SECONDS
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

    created_at = {}

    def create(path, resource):
        resource["metadata"]["ownerReferences"] = owner_ref
        name = resource["metadata"]["name"]
        specs[name] = resource
        try:
            result = request("POST", path, resource)
        except urllib.error.HTTPError as error:
            if error.code != 409:
                raise
            # A previous run's object with the same fixed name; replace it.
            result = replace_object(request, path, resource, deadline)
        created_at[name] = time.monotonic()
        track(path + "/" + result["metadata"]["name"])
        return result

    def track(path):
        if path not in created:
            created.append(path)

    def recreate(role):
        resource = specs[prefix + "-" + role]
        result = replace_object(request, pods, resource, deadline)
        created_at[prefix + "-" + role] = time.monotonic()
        track(pods + "/" + result["metadata"]["name"])
        return result

    def pod(role, code):
        spec = pod_spec(role, code)
        # Clients run in the sandbox runtime class, as sessions do.
        if role != "listener":
            if runtime_class:
                spec["spec"]["runtimeClassName"] = runtime_class
            if node_selector:
                spec["spec"]["nodeSelector"] = node_selector
            if tolerations:
                spec["spec"]["tolerations"] = tolerations
        return spec

    def pod_spec(role, code):
        return {
            "apiVersion": "v1", "kind": "Pod",
            "metadata": {"name": prefix + "-" + role,
                         "labels": {"alien.dev/cni-probe": prefix, "alien.dev/cni-role": role},
                         # Consolidation of an "underutilized" node would evict a probe pod mid-run.
                         "annotations": {"karpenter.sh/do-not-disrupt": "true"}},
            "spec": {
                "automountServiceAccountToken": False, "restartPolicy": "Never",
                "activeDeadlineSeconds": POD_DEADLINE_SECONDS, "terminationGracePeriodSeconds": 1,
                "securityContext": {"runAsNonRoot": True, "runAsUser": 65534,
                                    "seccompProfile": {"type": "RuntimeDefault"}},
                **({"imagePullSecrets": pull_secrets} if pull_secrets else {}),
                "containers": [{"name": "probe", "image": image,
                    "command": ["python3", "-B", "-c", code],
                    "terminationMessagePolicy": "FallbackToLogsOnError",
                    "securityContext": {"allowPrivilegeEscalation": False,
                                        "readOnlyRootFilesystem": True,
                                        "capabilities": {"drop": ["ALL"]}},
                    "resources": {"requests": {"cpu": "10m", "memory": "24Mi"},
                                  "limits": {"cpu": "100m", "memory": "128Mi"}}}],
            },
        }

    def wait(role, predicate):
        return wait_for_pod(request, pods, prefix, role, predicate, deadline,
                            recreate, FIX_HINT, created_at=created_at.get(prefix + "-" + role))

    # Pod and NetworkPolicy creation are confined by admission policies for this account.
    # Refuse to run unless each one rejects a request it must reject, with its own message.
    admission = os.environ["PROBE_ADMISSION_POLICY"]

    def require_denied(path, body, policy, message):
        while True:
            try:
                request("POST", path + "?dryRun=All", body)
            except urllib.error.HTTPError as error:
                text = error.read().decode(errors="replace")
                if f"ValidatingAdmissionPolicy '{policy}'" in text and message in text:
                    return
                raise RuntimeError(f"unexpected admission answer {error.code}: {text[:512]}")
            if time.monotonic() > admission_deadline:
                raise RuntimeError(f"admission policy {policy} is not enforced; refusing to run")
            time.sleep(1)

    escalation = pod_spec("admission-check", "pass")
    escalation["spec"]["automountServiceAccountToken"] = True
    require_denied(pods, escalation, admission, "probe pods carry no service account token")
    require_denied(policies, {
        "apiVersion": "networking.k8s.io/v1", "kind": "NetworkPolicy",
        "metadata": {"name": prefix + "-admission-check"},
        "spec": {"podSelector": {}, "policyTypes": ["Ingress"], "ingress": [{}]},
    }, admission + "-policies", "probe NetworkPolicies use the probe's own names")

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
        probe = client_code(url, marker)
        run_stages(
            lambda role, denied, recover=False: create(
                pods,
                pod(role, f"DENIED={denied!r}\n" + ("RECOVER=True\n" if recover else "") + probe),
            ),
            lambda role: wait(role, lambda p: p.get("status", {}).get("phase") == "Succeeded"),
        )
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
