"""Live race harness for GCP sandboxes keyed by a caller's own key. Not run by CI.

Each round starts N processes at one wall-clock instant, each calling `get_or_create(<same new
key>)` through the `alienplatform` Python binding (the way a deployed app reaches a sandbox). After
all return it checks, with a list taken straight from Google (not through the binding):
  1. every caller got an id, and all got the same one;
  2. exactly one caller saw created=True;
  3. exactly one live sandbox carries the key;
  4. that sandbox answers a command from a new process.
Then it terminates it from another process and checks `get(key)` and the list are empty.

    pip install alienplatform           # or the wheel built from the branch
    GCP_PROJECT=... GCP_REGION=... GCP_ENGINE=<engine id> GCP_TEMPLATE=<template id> \\
    GOOGLE_APPLICATION_CREDENTIALS=<federation or impersonation file; optional on a laptop with ADC> \\
      python3 gcp_sandbox_key_race.py --callers 3 --rounds 20 --parallel 2 --out race.jsonl

The engine and template must already exist (an image that carries the agent). Credentials come from
Application Default Credentials (`gcloud auth application-default login --impersonate-service-account=<email>`
on a laptop); no key file. The identity needs the Agent Platform sandbox permissions on the engine. Nothing secret is printed; results are
one JSON line per round with a hash of the key.
"""
import argparse, asyncio, base64, concurrent.futures as cf, hashlib, json, os, subprocess, sys, tempfile, time, urllib.parse, urllib.request, uuid

REQUIRED = ["GCP_PROJECT", "GCP_REGION", "GCP_ENGINE", "GCP_TEMPLATE"]
DEAD = ("STATE_DELETING", "STATE_DELETED", "STATE_TERMINATED", "STATE_STOPPED", "STATE_FAILED")


def cfg():
    missing = [n for n in REQUIRED if not os.environ.get(n)]
    if missing:
        sys.exit("set " + ", ".join(missing))
    engine = f"projects/{os.environ['GCP_PROJECT']}/locations/{os.environ['GCP_REGION']}/reasoningEngines/{os.environ['GCP_ENGINE']}"
    return engine, f"{engine}/sandboxEnvironmentTemplates/{os.environ['GCP_TEMPLATE']}"


async def worker(cmd, arg, barrier):
    engine, template = cfg()
    os.environ["ALIEN_DEPLOYMENT_TYPE"] = "gcp"
    os.environ["ALIEN_SBX_BINDING"] = json.dumps({
        "service": "sandbox-gcp-agent-platform", "engine": engine, "template": template,
        "region": os.environ["GCP_REGION"], "egress": "none"})
    from alienplatform import Sandbox
    sbx = Sandbox("sbx")
    if barrier:
        time.sleep(max(0, float(barrier) - time.time()))
    started = time.time()
    try:
        if cmd == "goc":
            info, created = await sbx.get_or_create(sandbox_id=arg)
            out = {"id": info.sandbox_id, "created": created}
        elif cmd == "get":
            info = await sbx.get(arg)
            out = None if info is None else {"id": info.sandbox_id, "state": str(info.state)}
        elif cmd == "terminate":
            await sbx.terminate(arg)
            out = {"terminated": True}
        elif cmd == "run":
            stream = await sbx.run_command(arg, "echo", ["ok"], timeout_ms=30000)
            frames = [repr(f) async for f in stream]
            out = {"answered": any("ok" in f for f in frames)}
        else:
            sys.exit("unknown command")
    except Exception as e:  # the class and code, not a traceback
        out = {"error": type(e).__name__, "code": getattr(e, "code", None), "message": str(e)[:200]}
    print(json.dumps({"seconds": round(time.time() - started, 1), "result": out}))


def drive(*args, barrier=None):
    cmd = [sys.executable, os.path.abspath(__file__), "--worker", *args] + ([str(barrier)] if barrier else [])
    return subprocess.Popen(cmd, stdout=subprocess.PIPE, text=True)


def output(proc):
    out, _ = proc.communicate()
    try:
        return json.loads(out.strip().splitlines()[-1])
    except Exception:
        return {"seconds": None, "result": {"error": "no-output"}}


def token():
    """An access token from Application Default Credentials."""
    return subprocess.run(["gcloud", "auth", "application-default", "print-access-token"],
                          capture_output=True, text=True, check=True).stdout.strip()


def listed(tok, key):
    """The engine's sandboxes carrying this key, straight from Google."""
    engine, _ = cfg()
    region = os.environ["GCP_REGION"]
    name = "alienRequestKey-" + hashlib.sha256(key.encode()).hexdigest()[:32]
    items, page = [], ""
    while True:
        url = f"https://{region}-aiplatform.googleapis.com/v1/{engine}/sandboxEnvironments?pageSize=100" + page
        req = urllib.request.Request(url, headers={"Authorization": "Bearer " + tok})
        data = json.load(urllib.request.urlopen(req))
        items += data.get("sandboxEnvironments", [])
        if not data.get("nextPageToken"):
            break
        page = "&pageToken=" + data["nextPageToken"]
    return [{"id": s["name"].rsplit("/", 1)[1], "state": s.get("state")} for s in items if s.get("displayName") == name]


def one_round(i, callers, tok):
    key = "engine-" + str(uuid.uuid4())
    barrier = time.time() + 8
    outs = [output(p) for p in [drive("goc", key, barrier=barrier) for _ in range(callers)]]
    res = [o["result"] for o in outs]
    ids = {r["id"] for r in res if "id" in r}
    created = sum(1 for r in res if r.get("created") is True)
    live = [x for x in listed(tok, key) if x["state"] not in DEAD]
    answered = output(drive("run", next(iter(ids))))["result"].get("answered") if ids else None
    ok = all("id" in r for r in res) and len(ids) == 1 and created == 1 and len(live) == 1 and answered is True
    for sid in ids:
        output(drive("terminate", sid))
    time.sleep(6)
    after_live = [x for x in listed(tok, key) if x["state"] not in DEAD]
    get_after = output(drive("get", key))["result"]
    return {"round": i, "callers": callers, "key_hash": hashlib.sha256(key.encode()).hexdigest()[:8],
            "verdict": "PASS" if ok and not after_live and get_after is None else "FAIL",
            "created_true": created, "distinct_ids": len(ids), "live_for_key": len(live), "answered_command": answered,
            "seconds": [o["seconds"] for o in outs], "errors": [r for r in res if "error" in r],
            "live_after_terminate": len(after_live), "get_key_after_terminate": get_after}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--callers", type=int, default=2)
    ap.add_argument("--rounds", type=int, default=10)
    ap.add_argument("--parallel", type=int, default=1)
    ap.add_argument("--out", default="race.jsonl")
    a = ap.parse_args()
    cfg()
    tok = token()  # valid for 50 minutes; rerun for longer batches
    fails = 0
    t0 = time.time()
    with open(a.out, "a") as f, cf.ThreadPoolExecutor(a.parallel) as ex:
        for r in ex.map(lambda i: one_round(i, a.callers, tok), range(a.rounds)):
            f.write(json.dumps(r) + "\n")
            f.flush()
            fails += r["verdict"] != "PASS"
        f.write(json.dumps({"summary": True, "callers": a.callers, "rounds": a.rounds, "fails": fails,
                            "wall_seconds": round(time.time() - t0)}) + "\n")
    print("rounds", a.rounds, "fails", fails)
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--worker":
        asyncio.run(worker(sys.argv[2], sys.argv[3] if len(sys.argv) > 3 else None, sys.argv[4] if len(sys.argv) > 4 else None))
    else:
        main()
