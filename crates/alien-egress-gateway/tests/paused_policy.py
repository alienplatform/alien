"""Pause the real agent while management narrows policy, then verify current authorization."""
import json
import time
from urllib.parse import urlencode


def check_paused_policy(suite, name, call, docker, aud, sub, uid):
    assert suite.admin("/v1/policies", {"aud":aud,"sub":sub,"policy":{"allow":["allowed.test","blocked.example.test"]}})[0] == 204
    assert call("PUT", "/v1/egress", {"token":suite.token(sub=sub)})[0] == 204
    old_version = suite.version(aud,sub)
    docker("pause",name)
    try:
        assert suite.admin("/v1/policies", {"aud":aud,"sub":sub,"policy":{"allow":["blocked.example.test"]}})[0] == 204
        assert json.loads(docker("inspect",name))[0]["State"]["Paused"]
    finally:
        docker("unpause",name)
    assert call("POST", "/aws/lambda-microvms/runtime/v1/resume", {})[0] == 200
    assert call("POST", "/v1/exec", {"command":["true"],"timeoutMs":1000})[0] == 200
    assert call("PUT", "/v1/egress", {"token":suite.token(sub=sub)})[0] == 204
    assert suite.version(aud,sub) > old_version
    probe = '''import socket,ssl,http.client
context=ssl.create_default_context(cafile="/fixture-ca.pem")
for host,allowed in [("allowed.test",False),("blocked.example.test",True)]:
 try:
  with context.wrap_socket(socket.create_connection((host,443),timeout=8),server_hostname=host) as s:
   s.sendall(("GET / HTTP/1.1\\r\\nHost: "+host+"\\r\\nConnection: close\\r\\n\\r\\n").encode())
   r=http.client.HTTPResponse(s);r.begin();assert r.status==200 and r.read()==b"controlled-origin-response\\n"
 except (ssl.SSLError,ConnectionError):
  assert not allowed,host
 else: assert allowed,host
print("paused policy delivery verified")
'''
    assert docker("exec","-u",f"{uid}:{uid}",name,"python","-c",probe) == "paused policy delivery verified"


def check_running_recovery(suite, call, docker, aud, sub, prefix):
    identity = {"aud":aud,"sub":sub}
    path = "/v1/policies/record?" + urlencode(identity)
    assert suite.admin("/v1/policies", {**identity,"policy":{"allow":["allowed.test","blocked.example.test"]}})[0] == 204
    token = suite.token(sub=sub)
    assert call("PUT", "/v1/egress", {"token":token})[0] == 204
    assert suite.admin("/v1/policies", {**identity,"policy":{"allow":["blocked.example.test"]}})[0] == 204
    status, body = suite.admin(path, identity, method="GET")
    assert status == 200
    retained = json.loads(body)["record"]
    assert call("PUT", "/v1/egress", {"policy":retained})[0] == 204
    replicas = [prefix+"-proxy1", prefix+"-proxy2"]
    docker("kill", *replicas)
    docker("start", *replicas)
    # No management write or agent delivery after loss: the restored signed bytes
    # witness what the real agent retained before both replicas lost their memory.
    deadline = time.monotonic() + 20
    for replica in ["93.184.216.3", "93.184.216.4"]:
        while True:
            try:
                status, body = suite.admin(path, identity, proxy=replica, method="GET")
                if status == 200:
                    assert json.loads(body)["record"] == retained
                    break
                assert status in [404,503], (status,body)
            except ConnectionError:
                pass
            assert time.monotonic() < deadline, "agent did not restore the latest record"
            time.sleep(.1)
        suite.tunnel(host="allowed.test",token=token,proxy=replica,expected=403).close()
        assert suite.tls(host="blocked.example.test",token=token,proxy=replica) == (200,b"controlled-origin-response\n")
    # A valid installed identity can resume; a record-only refresh preserves readiness.
    assert call("POST", "/aws/lambda-microvms/runtime/v1/resume", {})[0] == 200
    assert call("PUT", "/v1/egress", {"policy":retained})[0] == 204
    assert call("POST", "/v1/exec", {"command":["true"],"timeoutMs":1000})[0] == 200
    assert call("PUT", "/v1/egress", {"token":token,"policy":retained})[0] == 204
