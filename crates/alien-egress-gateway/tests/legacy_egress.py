"""Released object policies keep their direct, UID-filtered supervisor path."""
import json

from docker_conformance import ORIGIN, PREFIX, docker


def check_legacy_agents(suite, image, containers, uid):
    for index, mode in enumerate(["deny", "allow", "allowDomains"], 10):
        name = PREFIX + "-legacy-" + mode.lower()
        policy = {"mode": mode}
        if mode == "allowDomains":
            policy["domains"] = ["Allowed.TEST."]
        docker("run", "-d", "--name", name, "--network", PREFIX,
               "--ip", f"93.184.216.{index}", "--dns", ORIGIN, "--cap-add", "NET_ADMIN",
               "-e", "ALIEN_SANDBOX_EGRESS=" + json.dumps(policy), image)
        containers.append(name)
        docker("network", "connect", "--ip", f"10.77.91.{index}", PREFIX + "-private", name)
        script = r'''import base64,http.client,json,os,socket,threading,time
from pathlib import Path
MODE = MODE_VALUE
UID = UID_VALUE

def execute(program):
 c=http.client.HTTPConnection("127.0.0.1",8080,timeout=8)
 c.request("POST","/v1/exec",json.dumps({"command":["python","-c",program],"timeoutMs":5000}),{"Content-Type":"application/json"})
 r=c.getresponse();body=r.read();assert r.status==200,(r.status,body);c.close()
 frames=[json.loads(line) for line in body.splitlines()]
 output="".join(base64.b64decode(f["data"]).decode() for f in frames if f["t"] in ["stdout","stderr"])
 assert frames[-1]["t"]=="exit" and frames[-1]["code"]==0,(frames[-1],output)
 return output.strip()

deadline=time.monotonic()+8
while True:
 try:
  c=http.client.HTTPConnection("127.0.0.1",8080,timeout=1);c.request("GET","/v1/health");r=c.getresponse()
  assert r.status==200 and json.loads(r.read())["status"]=="ready"
  c.close()
  if Path("/data/image-env").exists():break
 except OSError:
  pass
 assert time.monotonic()<deadline,"legacy agent did not start without a gateway token"
 time.sleep(.05)
image_env=dict(line.split("=",1) for line in Path("/data/image-env").read_text().splitlines())
assert not image_env.get("HTTP_PROXY")
assert execute("import os;print(os.getuid())")==str(UID)
# These listeners are reachable from the namespace before testing the command UID's denials.
for host,port in [("93.184.216.2",80),("93.184.216.3",3128),("10.77.91.2",80)]:
 s=socket.create_connection((host,port),timeout=2)
 if port==80:
  s.sendall(b"GET / HTTP/1.1\r\nHost: allowed.test\r\nConnection: close\r\n\r\n")
  response=http.client.HTTPResponse(s);response.begin();assert response.status==200 and response.read()==b"controlled-origin-response\n"
 s.close()
for host,port,allowed in [("93.184.216.2",80,MODE!="deny"),("93.184.216.3",3128,MODE=="allow"),("10.77.91.2",80,False)]:
 program=f"""import socket
try:
 s=socket.create_connection(({host!r},{port}),timeout=.4);s.close();print('allowed')
except (TimeoutError,PermissionError):print('denied')"""
 assert execute(program)==("allowed" if allowed else "denied"),(MODE,host)
if MODE=="allowDomains":
 assert execute("import socket;print(socket.gethostbyname('allowed.test'),socket.gethostbyname('allowed.test.'))")=="93.184.216.2 93.184.216.2"
 assert "93.184.216.2 allowed.test allowed.test." in Path("/etc/hosts").read_text()
 assert execute("import os;print(os.access('/etc/hosts',os.W_OK))")=="False"
# The actual exec boundary denies IPv6 and raw packets, independently of image tools.
for expression in ["socket.socket(socket.AF_INET6,socket.SOCK_STREAM)","socket.socket(socket.AF_INET,socket.SOCK_RAW,socket.IPPROTO_TCP)"]:
 program=f"""import socket
try:{expression}
except PermissionError:print('blocked')
else:raise AssertionError('command bypassed socket restriction')"""
 assert execute(program)=="blocked"
def echo(sock):
 while True:
  data,peer=sock.recvfrom(1024);sock.sendto(data,peer)
for port in [53,5354]:
 sock=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);sock.bind(('127.0.0.1',port))
 threading.Thread(target=echo,args=(sock,),daemon=True).start()
for port in [53,5354]:
 program=f"""import socket
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.settimeout(.4)
try:s.sendto(b'echo',('127.0.0.1',{port}));print(s.recv(1024).decode())
except (TimeoutError,PermissionError):print('blocked')"""
 assert execute(program)==('echo' if port==5354 or MODE=='allow' else 'blocked'),(MODE,port)
print('legacy '+MODE+' passed without gateway or firewall tools')
'''.replace("MODE_VALUE", repr(mode)).replace("UID_VALUE", str(uid))
        def probe(name=name, script=script, mode=mode):
            actual = docker("exec", name, "python", "-c", script)
            assert actual == "legacy " + mode + " passed without gateway or firewall tools", actual
        suite.record("legacy " + mode + " keeps direct UID-filtered behavior", probe)
