"""System restore traffic stays outside command-UID routing and filtering."""
import json


def check_system_egress(suite, name, call, docker, uid, prefix):
    docker("network", "connect", "--ip", "10.77.91.50", prefix + "-private", name)
    system_uid = 991 if uid != 991 else 992
    program = '''import socket,http.client
assert socket.gethostbyname('gateway.test')=='172.29.91.2'
c=http.client.HTTPConnection('10.77.91.2',80,timeout=3)
c.request('GET','/');r=c.getresponse()
assert r.status==200 and r.read()==b'controlled-origin-response\\n'
c.close()
'''
    for identity in [0, system_uid]:
        suite.record(f"system UID {identity} keeps real DNS and private control traffic", lambda identity=identity: docker(
            "exec", "-u", f"{identity}:{identity}", name, "python", "-c", program))

    def guest():
        code = '''import socket,http.client
assert socket.gethostbyname('gateway.test').startswith('198.18.')
try:
 c=http.client.HTTPConnection('10.77.91.2',80,timeout=3)
 c.request('GET','/');r=c.getresponse()
 assert r.status!=200
except (OSError,http.client.HTTPException): pass
'''
        assert docker("exec", "-u", f"{uid}:{uid}", name, "python", "-c", code) == ""
    suite.record("command UID cannot use the system private-route exception", guest)
    # These hooks keep the installed rules. Local pause/unpause does not reproduce
    # AWS restore; the cloud run must still prove its two complete resume cycles.
    for hook in ["suspend", "resume"]:
        suite.record(f"agent {hook} hook remains reachable with routing installed", lambda hook=hook:
                     _hook(call, hook))
    status, body = call("POST", "/v1/exec", {"command": ["true"], "timeoutMs": 1000})
    assert status == 200, (status, body)
    frames = [json.loads(line) for line in body.splitlines()]
    assert frames[-1]["t"] == "exit" and frames[-1]["code"] == 0, frames
    assert call("PUT","/v1/egress",{"token":suite.token()})[0]==204


def _hook(call, hook):
    status, body = call("POST", "/aws/lambda-microvms/runtime/v1/" + hook, {})
    assert status == 200, (status, body)
