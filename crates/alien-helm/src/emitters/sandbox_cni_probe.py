"""Helm install gate: paired live traffic proves that ingress and egress NetworkPolicy work."""

import json
import os
import pathlib
import ssl
import time
import urllib.error
import urllib.request
import uuid


# The client's verdict when a denied pod reached the listener.
UNENFORCED_VERDICT = "NetworkPolicy did not block traffic"

# Shown with a verdict that NetworkPolicy is not enforced. It does not name the cluster the
# install is running on, so it lists the switches by provider.
FIX_HINT = (
    "Turn on NetworkPolicy enforcement in the cluster's network plugin and re-run the install. "
    "Examples: EKS Auto Mode: a ConfigMap amazon-vpc-cni in kube-system with "
    "enable-network-policy-controller: \"true\"; EKS with the VPC CNI add-on: enable network "
    "policy in the add-on configuration; GKE: Dataplane V2 or Calico; AKS: create the cluster "
    "with --network-policy azure|calico|cilium."
)

# The time budget of one run, stated here and only here. The chart reads JOB_DEADLINE_SECONDS from
# this file for the Job's activeDeadlineSeconds, and the script's deadline and every pod's
# activeDeadlineSeconds are SCRIPT_BUDGET_SECONDS. Helm's timeout (15m in every install path)
# applies to each hook, so the Job must fit in it with room for the next hook. A pod takes about
# 35 s to start when each pod is a VM (Kata). The listener runs on the sandbox nodes like the
# clients, and it is the first pod there: a node pool that scales from zero provisions a node for
# it, which takes up to NODE_SECONDS, and then the pod starts. The clients then land on that node.
# A run that passes needs:
#   ADMISSION_SECONDS         30   both admission policies answer
#   LISTENER_SECONDS         245   NODE_SECONDS for the node, then the listener starts and is ready
#   stage 1: 35 + 15               the control alone starts and finishes
#   stage 2: 35 + 15               denied and ingress-denied start together and finish
#   stage 3: 35 + 120 + 5          both controls start together; one may retry for RECOVER_SECONDS
#   one recreate: 45 + 35          a pod the runtime never started waits RUNTIME_START_BOUND_SECONDS
#   CLEANUP_SECONDS           10   delete what was created
# The runner's own wait for its network (API_READY_SECONDS) comes before this and counts in the Job.
API_READY_SECONDS = 60
ADMISSION_SECONDS = 30
POD_START_SECONDS = 35
NODE_SECONDS = 210
LISTENER_SECONDS = NODE_SECONDS + POD_START_SECONDS
CLIENT_RUN_SECONDS = POD_START_SECONDS + 15
RECOVER_SECONDS = 120
RECOVER_STAGE_SECONDS = POD_START_SECONDS + RECOVER_SECONDS + 5
RUNTIME_START_BOUND_SECONDS = 45
CLEANUP_SECONDS = 10
SCRIPT_BUDGET_SECONDS = (ADMISSION_SECONDS + LISTENER_SECONDS + 2 * CLIENT_RUN_SECONDS + RECOVER_STAGE_SECONDS
                         + RUNTIME_START_BOUND_SECONDS + POD_START_SECONDS + CLEANUP_SECONDS)
RUNNER_START_SECONDS = 30
BUDGET_MARGIN_SECONDS = 30
# The Job's deadline is API_READY + SCRIPT_BUDGET + RUNNER_START + BUDGET_MARGIN, written as a
# number because the chart reads it from this line (a test keeps the two equal).
JOB_DEADLINE_SECONDS = 745
# A pod that is still Pending after this long fails naming why. The listener and the clients all
# run on the sandbox nodes, so each may be the pod that waits for a node (a pool that scales from
# zero, or a node that is full and a second one is added).
PENDING_SECONDS = LISTENER_SECONDS
# The memory limit of a client pod. A pod whose runtime is a VM adds the limit to the VM's memory:
# 64Mi never started on AKS (Kata), 128Mi does.
CLIENT_MEMORY_LIMIT = "128Mi"

# The client pods' code. The caller sets DENIED and RECOVER before it. It writes one line to the
# pod's termination message; the runner reads that line and the exit code from the pod's status.
CLIENT = r'''
import errno,time,urllib.request,urllib.error
def require(condition,message):
    if not condition: raise RuntimeError(message)
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
DENIED=globals().get('DENIED',False)
RECOVER=globals().get('RECOVER',0)
LOG=globals().get('LOG','/dev/termination-log')
BLOCKED=(errno.ECONNREFUSED,errno.ENETUNREACH,errno.EHOSTUNREACH,errno.ETIMEDOUT)
def note(text):
    print(text,flush=True)
    try:
        with open(LOG,'w') as log: log.write(text[:2048])
    except OSError: pass
def run():
    # A recovering control waits, within the pod deadline, for a CNI that removes a deleted
    # policy slowly; it must still be reached once.
    begun=time.monotonic()
    reached=0
    attempts=0
    while True:
        attempts+=1
        try:
            body=opener.open(@URL@,timeout=3).read()
            require(body==@MARKER@, 'wrong positive control body')
            require(not DENIED, 'NetworkPolicy did not block traffic')
            reached+=1
            if RECOVER: break
        except (urllib.error.URLError,TimeoutError,OSError) as error:
            reason=getattr(error,'reason',error)
            require((DENIED or RECOVER) and (isinstance(reason,TimeoutError) or
                    getattr(reason,'errno',None) in BLOCKED), repr(error))
        if attempts>=3 and not RECOVER: break
        require(not RECOVER or time.monotonic()-begun<RECOVER, 'traffic did not resume after removing the policy')
        time.sleep(1)
    require(DENIED or reached, 'traffic did not resume after removing the policy')
    note('refused on all %d attempts'%attempts if DENIED else 'reached the listener on %d of %d attempts'%(reached,attempts))
try:
    run()
except BaseException as error:
    note('FAILED: %s'%error)
    raise
'''


def client_code(url, marker):
    """Source of a client pod; the caller prepends `DENIED=...` and `RECOVER=...`."""
    return CLIENT.replace("@URL@", repr(url)).replace("@MARKER@", repr(marker.encode()))


# A refusal is only meaningful bracketed by working controls, and a control that reached the
# listener proves the route to it is up from the sandbox nodes. So the control runs alone first and
# must succeed before any denied pod starts (a denied pod started earlier can be "refused" only
# because the route is not up yet, or because the CNI has not applied the policy to it yet). The
# two denied pods do not depend on each other and start together; so do the two controls after
# them. `ingress-denied` may leave its own node; the listener's ingress allow list is what refuses
# it. Entries are (role, denied, seconds the pod may retry for its first reply).
STAGES = [
    [("control", False, 0)],
    [("denied", True, 0), ("ingress-denied", True, 0)],
    [("control-after", False, 0), ("ingress-control-after", False, RECOVER_SECONDS)],
]

# A client pod whose probe container never ran says nothing about NetworkPolicy: the sandbox
# runtime could not start it. These are the container states the kubelet reports for that.
RUNTIME_START_REASONS = {
    "StartError", "ContainerCannotRun", "CreateContainerError", "RunContainerError",
}
IMAGE_PULL_REASONS = {"ErrImagePull", "ImagePullBackOff", "InvalidImageName", "ErrImageNeverPull"}


def _statuses(pod):
    return pod.get("status", {}).get("containerStatuses", [])


def terminated_message(pod):
    """The termination message of every container: the clients' one-line verdict."""
    return " ".join((s.get("state", {}).get("terminated") or {}).get("message", "")
                    for s in _statuses(pod)).strip()


def failure_detail(pod):
    """What the pod itself reports for a failure: the pod's reason (Evicted, DeadlineExceeded) and
    each container's terminated reason, exit code and message."""
    status = pod.get("status", {})
    parts = []
    if status.get("reason") or status.get("message"):
        parts.append("pod %s: %s" % (status.get("reason") or "failed", status.get("message") or "no message"))
    for container in _statuses(pod):
        terminated = (container.get("state", {}).get("terminated")
                      or container.get("lastState", {}).get("terminated"))
        if terminated:
            parts.append("container %s %s (exit code %s): %s" % (
                container.get("name", "probe"), terminated.get("reason") or "terminated",
                terminated.get("exitCode"), terminated.get("message") or "no termination message"))
    return "; ".join(parts) or "no termination message"


def runtime_start_failure(pod, pending_seconds):
    """The runtime's message when the probe container never ran, else None.

    A pod that ran is judged by its own verdict. A container the runtime did not start carries
    the runtime's reason and message instead: as a failed pod with no start time, or as a pod
    stuck waiting past RUNTIME_START_BOUND_SECONDS (the kubelet's own retries come first).
    """
    for status in _statuses(pod):
        state = status.get("state", {})
        terminated = state.get("terminated")
        if terminated:
            started = terminated.get("startedAt") or ""
            if (terminated.get("reason") in RUNTIME_START_REASONS
                    and (not started or started[:4] in ("0001", "1970"))):
                return terminated.get("message") or terminated.get("reason")
        waiting = state.get("waiting") or {}
        if waiting.get("reason") in RUNTIME_START_REASONS and pending_seconds > RUNTIME_START_BOUND_SECONDS:
            return waiting.get("message") or waiting["reason"]
    return None


def stuck_reason(pod):
    """The scheduler's or the kubelet's reason a pod has not started running, else None."""
    for condition in pod.get("status", {}).get("conditions", []):
        if condition.get("type") == "PodScheduled" and condition.get("status") == "False":
            return "not scheduled (%s): %s" % (condition.get("reason") or "Unschedulable",
                                               condition.get("message") or "no message")
    for status in _statuses(pod):
        waiting = status.get("state", {}).get("waiting") or {}
        if waiting.get("reason"):
            kind = "image did not pull" if waiting["reason"] in IMAGE_PULL_REASONS else "container waiting"
            return "%s (%s): %s" % (kind, waiting["reason"], waiting.get("message") or "no message")
    return None


def replace_object(request, path, resource, deadline, now=time.monotonic, sleep=time.sleep):
    """Delete an object, wait until it is gone, create it again.

    A pod deleted with no grace period can stay in the API for a moment; creating the same name
    then answers 409, so the new object is only created once the old one answers 404.
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


def wait_for_pod(request, pods, prefix, role, predicate, deadline, recreate,
                 now=time.monotonic, sleep=time.sleep, out=print, created_at=None,
                 pending_bound=PENDING_SECONDS):
    """Wait for a pod to satisfy `predicate`. The decision is the pod's status.

    A pod that ran is judged by its termination message and exit code. A pod the runtime never
    started is recreated once and named as such. A pod Pending for longer than `pending_bound`
    fails naming the scheduler's or the kubelet's reason. Each failure names the pod.
    `created_at` is when the pod was created: the pods of a stage are created together and
    waited for one by one, so a pod's stuck time counts from its creation.
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
                raise RuntimeError(f"the runtime failed to start the {role} pod twice: {failure[:512]}")
            out(f"the runtime failed to start the {role} pod: {failure[:512]}; recreated", flush=True)
            recreate(role)
            recreated = True
            first_seen = now()
            sleep(1)
            continue
        if phase == "Failed":
            verdict = terminated_message(current)
            if UNENFORCED_VERDICT in verdict:
                # The one verdict that says the cluster does not enforce NetworkPolicy: a denied
                # pod reached the listener. Only this one carries the fix hint.
                raise RuntimeError(f"the {role} pod reached the listener it must not reach: {verdict[:512]} | "
                                   + FIX_HINT)
            raise RuntimeError(f"the {role} pod failed (NetworkPolicy enforcement is unproven): "
                               + failure_detail(current)[:1024])
        if predicate(current):
            verdict = terminated_message(current)
            out(f"{role}: {phase}; node={current['spec'].get('nodeName')}; "
                f"ip={current.get('status', {}).get('podIP')}" + (f"; verdict: {verdict[:256]}" if verdict else ""),
                flush=True)
            return current
        reason = stuck_reason(current) if phase == "Pending" else None
        if reason and now() - first_seen > pending_bound:
            raise RuntimeError(f"the {role} pod never ran, so NetworkPolicy was not tested: {reason}")
        sleep(1)
    reason = stuck_reason(current) or "phase " + str(current.get("status", {}).get("phase"))
    raise RuntimeError(f"the {role} pod did not finish before the probe's deadline ({reason}); "
                       "NetworkPolicy enforcement is unproven")


def main():
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

    run_probe(request, os.environ)


def run_probe(request, env, now=time.monotonic, sleep=time.sleep, out=print):
    """The whole run against an API (`request`) and the environment; the clock is a parameter so
    a test can drive it against a fake cluster."""
    namespace = env["PROBE_NAMESPACE"]
    job = env["PROBE_JOB"]
    image = env["PROBE_IMAGE"]
    pull_secrets = [{"name": n} for n in env.get("PROBE_PULL_SECRETS", "").split(",") if n]
    pods = f"/api/v1/namespaces/{namespace}/pods"
    policies = f"/apis/networking.k8s.io/v1/namespaces/{namespace}/networkpolicies"
    # A cluster that denies pod traffic by default admits this pod's API calls only once the
    # CNI has applied the runner policy, shortly after the pod starts.
    start = now()
    while True:
        try:
            owner = request("GET", f"/apis/batch/v1/namespaces/{namespace}/jobs/{job}")
            break
        except (urllib.error.URLError, TimeoutError) as error:
            if isinstance(error, urllib.error.HTTPError) or now() - start > 60:
                raise
            sleep(2)
    owner_ref = [{"apiVersion": "batch/v1", "kind": "Job", "name": job,
                  "uid": owner["metadata"]["uid"]}]
    # Fixed names: the probe's Role may read and delete only these objects.
    prefix = job
    runtime_class = env["PROBE_RUNTIME_CLASS"]
    # Where sessions run (`sandbox.nodeSelector`, `sandbox.tolerations`): the probe's clients must
    # be tested on those nodes, not on whichever node the scheduler prefers.
    node_selector = json.loads(env.get("PROBE_NODE_SELECTOR") or "{}")
    tolerations = json.loads(env.get("PROBE_TOLERATIONS") or "[]")
    created = []
    # The node of the first control, once it ran. A refusal only counts on a node where a control
    # reached the listener, so every later client is pinned to it.
    pinned_node = []
    deadline = now() + SCRIPT_BUDGET_SECONDS
    marker = uuid.uuid4().hex
    server = (
        "from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer\n"
        "class Handler(BaseHTTPRequestHandler):\n"
        " def do_GET(self):\n"
        "  self.send_response(200); self.end_headers(); "
        f"self.wfile.write({marker.encode()!r})\n"
        " def log_message(self,*args): pass\n"
        "ThreadingHTTPServer(('0.0.0.0',8080),Handler).serve_forever()\n"
    )

    def create(path, resource):
        resource["metadata"]["ownerReferences"] = owner_ref
        try:
            result = request("POST", path, resource)
        except urllib.error.HTTPError as error:
            if error.code != 409:
                raise
            # A previous run's object with the same fixed name; replace it.
            result = replace_object(request, path, resource, deadline, now, sleep)
        created.append(path + "/" + result["metadata"]["name"])
        return result

    def pod(role, code):
        spec = pod_spec(role, code)
        # Every pod runs where sessions run, in the sandbox runtime class: the listener tests
        # ingress into a sandbox pod, and the clients share its node.
        if runtime_class:
            spec["spec"]["runtimeClassName"] = runtime_class
        if node_selector or pinned_node:
            spec["spec"]["nodeSelector"] = {**node_selector, **({"kubernetes.io/hostname": pinned_node[0]}
                                                                 if pinned_node else {})}
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
                "activeDeadlineSeconds": SCRIPT_BUDGET_SECONDS, "terminationGracePeriodSeconds": 1,
                "securityContext": {"runAsNonRoot": True, "runAsUser": 65534,
                                    "seccompProfile": {"type": "RuntimeDefault"}},
                **({"imagePullSecrets": pull_secrets} if pull_secrets else {}),
                "containers": [{"name": "probe", "image": image,
                    "command": ["python3", "-B", "-c", code],
                    "securityContext": {"allowPrivilegeEscalation": False,
                                        "readOnlyRootFilesystem": True,
                                        "capabilities": {"drop": ["ALL"]}},
                    "resources": {"requests": {"cpu": "10m", "memory": "24Mi"},
                                  "limits": {"cpu": "100m", "memory": CLIENT_MEMORY_LIMIT}}}],
            },
        }

    created_at = {}
    specs = {}

    def start(role, spec):
        specs[role] = spec
        create(pods, spec)
        created_at[role] = now()

    def recreate(role):
        replace_object(request, pods, specs[role], deadline, now, sleep)
        created_at[role] = now()

    def wait(role, predicate, pending_bound=PENDING_SECONDS):
        return wait_for_pod(request, pods, prefix, role, predicate, deadline, recreate,
                            created_at=created_at.get(role), pending_bound=pending_bound,
                            now=now, sleep=sleep, out=out)

    # Pod and NetworkPolicy creation are confined by admission policies for this account.
    # Refuse to run unless each one rejects a request it must reject, with its own message.
    admission = env["PROBE_ADMISSION_POLICY"]

    admission_until = now() + ADMISSION_SECONDS

    def require_denied(path, body, policy, message):
        while True:
            try:
                request("POST", path + "?dryRun=All", body)
            except urllib.error.HTTPError as error:
                text = error.read().decode(errors="replace")
                if f"ValidatingAdmissionPolicy '{policy}'" in text and message in text:
                    return
                raise RuntimeError(f"unexpected admission answer {error.code}: {text[:512]}")
            if now() > admission_until:
                raise RuntimeError(f"admission policy {policy} is not enforced; refusing to run")
            sleep(1)

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
        start("listener", listener)
        listener = wait("listener", lambda p: any(
            c["type"] == "Ready" and c["status"] == "True"
            for c in p.get("status", {}).get("conditions", [])))
        ip = listener["status"]["podIP"]
        authority = f"[{ip}]" if ":" in ip else ip
        code = client_code(f"http://{authority}:8080/", marker)
        succeeded = lambda p: p.get("status", {}).get("phase") == "Succeeded"
        for stage in STAGES:
            for role, denied, recover in stage:
                start(role, pod(role, f"DENIED={denied!r}\nRECOVER={recover!r}\n" + code))
            for role, _, _ in stage:
                finished = wait(role, succeeded)
                node = finished.get("spec", {}).get("nodeName")
                if not pinned_node:
                    if not node:
                        raise RuntimeError(f"the {role} pod reports no node, so the later pods cannot be "
                                           "placed on a node where a control reached the listener")
                    pinned_node.append(node)
                elif node != pinned_node[0]:
                    raise RuntimeError(f"the {role} pod ran on node {node}, not on {pinned_node[0]} where the "
                                       "control reached the listener; NetworkPolicy enforcement is unproven")
        out("NetworkPolicy enforced: ingress and egress denied requests; all controls passed")
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
