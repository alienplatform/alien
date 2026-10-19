"""Measure the accepted proxy-only DNS channel and the privileged DNS redirect."""
import base64
import http.client
import json
import time
from pathlib import Path

# This runs through the agent's real exec endpoint, under the configured command UID.
PROBE = r'''
import base64,json,socket,struct,sys,time
resolver,mode,count=sys.argv[1],sys.argv[2],int(sys.argv[3])
payload=base64.b32encode(bytes(range(96))).decode().rstrip("=").lower()
payload_labels=[payload[i:i+50] for i in range(0,len(payload),50)]
def read(stream,n):
 data=bytearray()
 while len(data)<n:
  part=stream.recv(n-len(data))
  assert part,"short DNS frame"
  data.extend(part)
 return bytes(data)
results=[]
for transport in ["udp","tcp"]:
 names=[];wire_bytes=0;codes={};started=time.monotonic()
 for sequence in range(count):
  name=".".join([mode,transport,f"{sequence:08x}",*payload_labels,"probe","invalid"])
  question=b"".join(bytes([len(x)])+x.encode() for x in name.split("."))+b"\0\0\1\0\1"
  packet=struct.pack("!HHHHHH",sequence,0x100,1,0,0,0)+question
  with socket.socket(socket.AF_INET,socket.SOCK_DGRAM if transport=="udp" else socket.SOCK_STREAM) as stream:
   stream.settimeout(2)
   if transport=="udp":
    stream.sendto(packet,(resolver,53));response=stream.recv(4096)
   else:
    stream.connect((resolver,53));stream.sendall(struct.pack("!H",len(packet))+packet)
    response=read(stream,struct.unpack("!H",read(stream,2))[0])
  assert response[:2]==packet[:2],"DNS transaction mismatch"
  code=response[3]&15;codes[str(code)]=codes.get(str(code),0)+1
  names.append(name);wire_bytes+=len(packet)+(2 if transport=="tcp" else 0)
  time.sleep(max(0,started+(sequence+1)/100-time.monotonic()))
 elapsed=time.monotonic()-started
 results.append(dict(mode=mode,transport=transport,count=count,elapsedSeconds=elapsed,
                     queriesPerSecond=count/elapsed,payloadBytes=count*96,
                     payloadBytesPerSecond=count*96/elapsed,queryBytes=wire_bytes,
                     queryBytesPerSecond=wire_bytes/elapsed,responseCodes=codes,names=names))
print(json.dumps(results))
'''


def check_dns_limit(suite, docker, containers, prefix):
    directory = suite.output / "resolver-measurement"
    directory.mkdir(mode=0o777)
    directory.chmod(0o777)
    name = prefix + "-dns-measure"
    docker("run", "-d", "--name", name, "--network", prefix + "-guest", "--ip", "172.29.91.5",
           "--cap-drop", "ALL", "--read-only", "-v", f"{directory}:/measure",
           "-v", f"{Path(__file__).with_name('dns_measure_resolver.py')}:/resolver.py:ro",
           "python:3.13-slim", "python", "/resolver.py")
    containers.append(name)
    deadline = time.monotonic() + 5
    while not (directory / "ready").exists():
        assert time.monotonic() < deadline, "controlled resolver did not start"
        time.sleep(.05)

    def execute(address, mode, count=128):
        connection = http.client.HTTPConnection(address, 8080, timeout=30)
        try:
            connection.request("POST", "/v1/exec", json.dumps({
                "command": ["python", "-c", PROBE, "172.29.91.5", mode, str(count)],
                "timeoutMs": 20000,
            }), {"Content-Type": "application/json"})
            response = connection.getresponse()
            body = response.read()
            assert response.status == 200, (response.status, body)
        finally:
            connection.close()
        frames = [json.loads(line) for line in body.splitlines()]
        assert frames[-1]["t"] == "exit" and frames[-1]["code"] == 0, frames
        output = b"".join(base64.b64decode(frame["data"]) for frame in frames if frame["t"] == "stdout")
        return json.loads(output)

    measurements = []
    for address, mode in [("172.29.91.4", "proxy"), ("172.29.91.3", "privileged"),
                          ("172.29.91.4", "control")]:
        measurements.extend(execute(address, mode, 1 if mode == "control" else 128))
    received = [json.loads(line) for line in (directory / "queries.jsonl").read_text().splitlines()]
    for result in measurements:
        matches = [entry for entry in received if entry["name"] in result["names"]]
        result["receivedQueries"] = len(matches)
        result["receivedQueryBytes"] = sum(entry["queryBytes"] for entry in matches)
    (suite.output / "dns-resolver-limit.json").write_text(json.dumps({
        "backend": "controlled local Docker network with a reachable UDP/TCP resolver",
        "rateLimitQueriesPerSecond": 100,
        "payloadBytesPerName": 96,
        "note": "A configured test rate, not a measured maximum or a cloud-provider throughput claim.",
        "measurements": measurements,
    }, indent=2))
    def forwarded():
        for result in measurements:
            if result["mode"] != "privileged":
                assert result["receivedQueries"] == result["count"], result
                assert result["responseCodes"] == {"3": result["count"]}, result

    def synthetic():
        for result in measurements:
            if result["mode"] == "privileged":
                assert result["receivedQueries"] == 0, result
                assert result["responseCodes"] == {"0": result["count"]}, result

    suite.record("proxy-only UDP/TCP resolver carries fake names outside gateway policy", forwarded)
    suite.record("privileged UDP/TCP DNS stays synthetic; resolver receives zero names", synthetic)
