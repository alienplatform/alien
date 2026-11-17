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
# (sandbox_cni.rs reads JOB_DEADLINE_SECONDS from this file) come from these numbers; the Helm
# install's own wait (`--timeout 15m` in the install instructions) must fit the Job with room for
# the next hook. A pod start is ~35 s under a VM-per-pod runtime (Kata). A run that passes needs:
#   API_READY_SECONDS       60   the runner's own network is applied (the wait below)
#   ADMISSION_SECONDS       30   both admission policies answer
#   POD_START_SECONDS       35   the listener starts
#   stage 1: 35 + 35 + 85 + 6    the denied pods start, then the control (which needs their
#                                addresses) starts, then the control runs until the denied pods
#                                are done (CONTROL_RUN_SECONDS), which linger 6 s (LINGER_SECONDS)
#   stage 2: 35 + 33             the controls start together; the recovering one waits up to 30 s
#   one recreate            45 + 35   (START_BOUND_SECONDS stuck, then a new start)
#   CLEANUP_SECONDS         10   delete what was created
#                          ---
#                          444
# The Job adds the runner's own start (image pull, 30 s) and a 30 s margin: 504 s. The install's
# 15 minutes leave 396 s for the hooks and the rollout after it.
API_READY_SECONDS = 60
ADMISSION_SECONDS = 30
POD_START_SECONDS = 35
# A denied pod: how long it waits for the control to reach it (the control's own pod start, then
# one request), how long a refusal may take to appear, and how long refusals must then hold.
DENIED_SERVED_WAIT_SECONDS = 60
ENFORCEMENT_WINDOW_SECONDS = 45
HOLD_SECONDS = 10
# After it is done a denied pod keeps answering, so the control can read that it is done.
LINGER_SECONDS = 6
# The control: how long it may take to reach each denied pod once, and its whole run: until every
# denied pod has finished, which is at most served + window + hold after the control's start.
SERVE_WAIT_SECONDS = 20
CONTROL_RUN_SECONDS = SERVE_WAIT_SECONDS + ENFORCEMENT_WINDOW_SECONDS + HOLD_SECONDS + 10
# A control after the policies are removed waits this long for the CNI to remove a deleted one.
RECOVER_SECONDS = 30
RECOVER_RUN_SECONDS = RECOVER_SECONDS + 3
STAGE_ONE_SECONDS = 35 + 35 + CONTROL_RUN_SECONDS + LINGER_SECONDS
STAGE_TWO_SECONDS = 35 + RECOVER_RUN_SECONDS
CLEANUP_SECONDS = 10
# How long a pod may stay Pending with a container the runtime failed to create, before it is
# treated as not started and recreated. A retry the kubelet makes by itself succeeds within its
# first backoff steps (10 s, then 20 s), so waiting longer than this is not worth the budget.
START_BOUND_SECONDS = 45
# How long a pod may stay Pending, unschedulable or failing to pull its image, before the run
# fails naming that cause. The cause does not change by recreating the pod, so there is no retry.
# A node that has to be provisioned for the pod is part of POD_START_SECONDS.
PENDING_CAUSE_BOUND_SECONDS = 90
SCRIPT_BUDGET_SECONDS = 444
RUNNER_START_SECONDS = 30
BUDGET_MARGIN_SECONDS = 30
JOB_DEADLINE_SECONDS = 504
# Each client pod's own deadline, counted by the kubelet from the pod's start.
POD_DEADLINE_SECONDS = 240


CLIENT = r'''
import errno,http.server,socket,threading,time,urllib.request,urllib.error
def require(condition,message):
    if not condition: raise RuntimeError(message)
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))
DENIED=globals().get('DENIED',False)
RECOVER=globals().get('RECOVER',0)
TARGETS=globals().get('TARGETS',[])
LOG=globals().get('LOG','/dev/termination-log')
BIND=globals().get('BIND','0.0.0.0')
PORT=globals().get('PORT',8080)
WINDOW=globals().get('WINDOW',@WINDOW@)
HOLD=globals().get('HOLD',@HOLD@)
LINGER=globals().get('LINGER',@LINGER@)
SERVED_WAIT=globals().get('SERVED_WAIT',@SERVED_WAIT@)
SERVE_WAIT=globals().get('SERVE_WAIT',@SERVE_WAIT@)
CONTROL_RUN=globals().get('CONTROL_RUN',@CONTROL_RUN@)
BLOCKED=(errno.ECONNREFUSED,errno.ENETUNREACH,errno.EHOSTUNREACH,errno.ETIMEDOUT)
def get(url):
    # The body when the peer answered, None when the network refused or dropped the request.
    try:
        return opener.open(url,timeout=3).read()
    except (urllib.error.URLError,TimeoutError,OSError) as error:
        reason=getattr(error,'reason',error)
        require(isinstance(reason,TimeoutError) or getattr(reason,'errno',None) in BLOCKED, repr(error))
        return None
def reach():
    body=get(@URL@)
    require(body is None or body==@MARKER@, 'wrong positive control body')
    return body is not None
def note(text):
    print(text,flush=True)
    try:
        with open(LOG,'w') as log: log.write(text)
    except OSError: pass
if DENIED:
    # A denied pod serves too (its policy is egress-only), so the control can prove that this
    # pod's own network is up; a refusal only counts once it has been reached.
    state={'served':False,'state':'waiting'}
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            state['served']=True
            self.send_response(200); self.end_headers()
            self.wfile.write(@MARKER@+(b' '+state['state'].encode() if self.path=='/state' else b''))
        def log_message(self,*args): pass
    server=http.server.ThreadingHTTPServer((BIND,PORT),Handler)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    try:
        waited=time.monotonic()
        while not state['served']:
            require(time.monotonic()-waited<SERVED_WAIT,'denied pod network is not up: the control did not reach it')
            time.sleep(0.2)
        state['state']='counting'
        # The policy may need a moment to reach a new pod: wait, bounded, for the first refusal, then
        # require refusals to hold. Reaching the listener after a refusal, or never being refused
        # within the window, is a failure.
        start=time.monotonic()
        reachable=0
        while reach():
            reachable+=1
            require(time.monotonic()-start<WINDOW,'NetworkPolicy did not block traffic')
            time.sleep(1)
        blocked_after=time.monotonic()-start
        hold_until=time.monotonic()+HOLD
        while time.monotonic()<hold_until:
            time.sleep(1)
            require(not reach(),'NetworkPolicy did not block traffic: reachable again after a refusal')
        state['state']='done'
        note('blocked after %.1fs (%d reachable attempts first), held %ds'%(blocked_after,reachable,HOLD))
        time.sleep(LINGER)
    finally:
        server.shutdown(); server.server_close()
elif TARGETS:
    # The positive control of the denied pods: it reaches the listener every second for as long as
    # they count refusals, so a refusal cannot be an outage of the listener. It reaches each denied
    # pod once, which starts that pod's count, and stops when every one of them says it is done.
    seen=set(); done=set()
    begun=time.monotonic()
    while len(done)<len(TARGETS):
        require(time.monotonic()-begun<CONTROL_RUN,'denied pods did not finish in time')
        try:
            up=reach()
        except RuntimeError:
            up=False
        require(up,'listener unreachable from the control: a refusal proves nothing')
        for target in TARGETS:
            if target in done: continue
            try:
                body=get(target+'state')
            except RuntimeError:
                body=None
            if body is None:
                require(target not in seen and time.monotonic()-begun<SERVE_WAIT,
                        'denied pod unreachable from the control: its own network is not up')
            else:
                seen.add(target)
                require(body.startswith(@MARKER@),'wrong denied pod body')
                if body.endswith(b' done'): done.add(target)
        time.sleep(1)
    note('listener reachable throughout; %d denied pods finished'%len(TARGETS))
elif RECOVER:
    # A recovering control waits, within the pod deadline, for a CNI that removes a deleted
    # policy slowly; it must still be reached once.
    until=time.monotonic()+RECOVER
    reached=reach()
    while not reached and time.monotonic()<until:
        time.sleep(1)
        reached=reach()
    require(reached,'traffic did not resume after removing the policy')
else:
    for _ in range(3):
        require(reach(),'positive control was not reached')
        time.sleep(1)
'''


def client_code(url, marker):
    """Source of the client pods. The caller sets `DENIED`, `RECOVER` or `TARGETS` before it."""
    constants = {
        "@WINDOW@": ENFORCEMENT_WINDOW_SECONDS, "@HOLD@": HOLD_SECONDS, "@LINGER@": LINGER_SECONDS,
        "@SERVED_WAIT@": DENIED_SERVED_WAIT_SECONDS, "@SERVE_WAIT@": SERVE_WAIT_SECONDS,
        "@CONTROL_RUN@": CONTROL_RUN_SECONDS, "@URL@": repr(url), "@MARKER@": repr(marker.encode()),
    }
    source = CLIENT
    for token, value in constants.items():
        source = source.replace(token, str(value))
    return source


# A successful negative result is only meaningful bracketed by working controls, and a refusal
# only counts while the listener is shown to be up and the denied pod's own network is shown to
# be up. Stage 1: the denied pods start first and serve; once they run, the control starts with
# their addresses, reaches the listener every second for as long as they count, and reaches each
# denied pod once. The `ingress-denied` pod may leave its own node; the listener's ingress allow
# list is what refuses it. Stage 2 starts its two controls together: the recovering one waits for
# the CNI to remove a deleted policy.
DENIED_ROLES = ["denied", "ingress-denied"]
CONTROL_ROLE = "control"
AFTER_ROLES = [("control-after", 0), ("ingress-control-after", RECOVER_SECONDS)]


def run_stages(start_client, wait_client, wait_running):
    """`start_client(role, **settings)` creates a pod; `wait_running(role)` returns it Running."""
    for role in DENIED_ROLES:
        start_client(role, DENIED=True)
    addresses = [wait_running(role)["status"]["podIP"] for role in DENIED_ROLES]
    start_client(CONTROL_ROLE, TARGETS=addresses)
    for role in [CONTROL_ROLE, *DENIED_ROLES]:
        wait_client(role)
    for role, recover in AFTER_ROLES:
        start_client(role, RECOVER=recover)
    for role, _ in AFTER_ROLES:
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
            # The kubelet reports an unstarted container with a zero or missing start time, an
            # exit code of 128 or a start-failure reason; a probe that ran exits 0 or 1.
            never_started = (terminated.get("reason") in RUNTIME_START_REASONS
                             or terminated.get("exitCode") == 128
                             or not started or started[:4] in ("0001", "1970"))
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
                 now=time.monotonic, sleep=time.sleep, out=print):
    """Wait for a pod to satisfy `predicate`, recreating it once if the runtime failed to start it."""
    first_seen = now()
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

    # The probe pods run where sandboxes run: `sandbox.namespace` when the chart sets one, else
    # the release namespace. The runner and its Job stay in the release namespace.
    pod_namespace = os.environ.get("PROBE_POD_NAMESPACE") or namespace
    pods = f"/api/v1/namespaces/{pod_namespace}/pods"
    policies = f"/apis/networking.k8s.io/v1/namespaces/{pod_namespace}/networkpolicies"
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
    # An owner must live in the object's own namespace: in another one the garbage collector would
    # delete the object at once. Then the run's own cleanup is what removes them.
    owner_ref = ([{"apiVersion": "batch/v1", "kind": "Job", "name": job,
                   "uid": owner["metadata"]["uid"]}] if pod_namespace == namespace else [])
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

    def create(path, resource):
        if owner_ref:
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
        track(path + "/" + result["metadata"]["name"])
        return result

    def track(path):
        if path not in created:
            created.append(path)

    def recreate(role):
        resource = specs[prefix + "-" + role]
        result = replace_object(request, pods, resource, deadline)
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
                            recreate, FIX_HINT)

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
        def start(role, **settings):
            settings_code = "".join(f"{name}={value!r}\n" for name, value in settings.items())
            create(pods, pod(role, settings_code + probe))

        run_stages(
            start,
            lambda role: wait(role, lambda p: p.get("status", {}).get("phase") == "Succeeded"),
            lambda role: wait(role, lambda p: p.get("status", {}).get("phase") == "Running"
                              and bool(p["status"].get("podIP"))),
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
