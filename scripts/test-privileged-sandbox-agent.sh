#!/usr/bin/env bash
# Behavioral qualification in a private Docker network namespace; no host netfilter changes.
set -euo pipefail
[ "$#" -eq 1 ] || { echo "usage: $0 <native-agent-binary>" >&2; exit 2; }
agent_binary=$(realpath "$1")
fixture=$(mktemp -d)
container="probe-alien1188-agent-test-$$"
image="probe-alien1188-agent-test:$$"
container_created=false
image_created=false
cleanup() {
  result=$?
  trap - EXIT
  if "$container_created"; then docker rm -f "$container" >/dev/null || result=1; fi
  if "$image_created"; then docker image rm "$image" >/dev/null || result=1; fi
  rm -rf "$fixture" || result=1
  exit "$result"
}
trap cleanup EXIT
cat > "$fixture/Dockerfile" <<'DOCKERFILE'
FROM public.ecr.aws/docker/library/buildpack-deps:26.04@sha256:159ea382e6fb39e62480ee932113f885f7bd787cd4895fc4dc71aebb175077fd
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends iptables python3 && rm -rf /var/lib/apt/lists/*
RUN mkdir -p /sandbox /opt/alien && chown 60000:60000 /sandbox && chmod 0700 /sandbox
DOCKERFILE
cat > "$fixture/image-command.json" <<'JSON'
{"command":["/bin/sh","-c","id -u > /sandbox/image-uid; grep '^CapEff:' /proc/self/status >> /sandbox/image-uid"],"env":{"PATH":"/usr/bin:/bin"},"workingDirectory":"/"}
JSON
docker build -q -t "$image" "$fixture" >/dev/null
image_created=true
docker run -d --name "$container" --cap-add=NET_ADMIN \
  -v "$agent_binary:/usr/local/bin/alien-sandbox-agent:ro" \
  -v "$fixture/image-command.json:/opt/alien/image-command.json:ro" \
  -e ALIEN_SANDBOX_ROOT=/sandbox -e ALIEN_SANDBOX_PORT=8971 \
  -e ALIEN_SANDBOX_AUTHORIZATION=transport -e ALIEN_SANDBOX_ISOLATION=uid-split \
  -e ALIEN_SANDBOX_EXEC_UID=60001 -e ALIEN_SANDBOX_EXEC_GID=60001 \
  -e 'ALIEN_SANDBOX_EGRESS={"mode":"allowDomains","domains":["Example.COM."]}' \
  --entrypoint /usr/local/bin/alien-sandbox-agent "$image" >/dev/null
container_created=true
# The test driver is external to the supervisor. Its root identity permits transport-mode calls;
# every command it submits still passes through the agent's unprivileged spawn boundary.
docker exec -i "$container" python3 - <<'PY'
import base64,json,os,socket,threading,time,urllib.request
opener=urllib.request.build_opener(urllib.request.ProxyHandler({}))

def execute(command,timeout=10000):
    request=urllib.request.Request('http://127.0.0.1:8971/v1/exec',data=json.dumps({
        'command':command,'timeoutMs':timeout,'env':{'ALIEN_SANDBOX_EXEC_UID':'0','ALIEN_SANDBOX_EGRESS':'{"mode":"allow"}'}}).encode(),headers={'Content-Type':'application/json'})
    frames=[json.loads(line) for line in opener.open(request,timeout=20).read().splitlines()]
    output=''.join(base64.b64decode(frame['data']).decode() for frame in frames if frame['t'] in ['stdout','stderr'])
    assert frames[-1]['t'] in ['exit','error'],frames
    return output,frames[-1]

for attempt in range(50):
    try:
        opener.open('http://127.0.0.1:8971/v1/health',timeout=1).close(); break
    except OSError:
        if attempt==49: raise
        time.sleep(.1)

out,terminal=execute(['/bin/sh','-c','id -u; grep "^Cap" /proc/self/status; cat image-uid'])
assert terminal['code']==0 and out.startswith('60001\n'),(out,terminal)
assert 'CapEff:\t0000000000000000' in out and 'CapBnd:\t0000000000000000' in out,out
assert out.endswith('60001\nCapEff:\t0000000000000000\n'),out
for command in [['/usr/sbin/iptables-nft','-P','OUTPUT','ACCEPT'],['/bin/sh','-c','echo overwritten >> /etc/hosts']]:
    out,terminal=execute(command); assert terminal['t']=='exit' and terminal['code']!=0,(command,out,terminal)
# This qualifies routing, independent of the test host's TLS interception trust store.
for hostname in ['example.com']:
    out,terminal=execute(['/usr/bin/curl','-4','-k','-sS','-o','/dev/null','-w','%{http_code}','--max-time','8','https://'+hostname])
    assert out=='200' and terminal['code']==0,(hostname,out,terminal)
out,terminal=execute(['/usr/bin/python3','-c',"import socket; a={x[4][0] for x in socket.getaddrinfo('example.com',443,socket.AF_INET)}; b={x[4][0] for x in socket.getaddrinfo('example.com.',443,socket.AF_INET)}; print(bool(a) and a==b)"])
assert out.strip()=='True' and terminal['code']==0,(out,terminal)
out,terminal=execute(['/usr/bin/curl','-4','-k','-sS','-o','/dev/null','--max-time','2','https://1.1.1.1'])
assert terminal['code']==28,(out,terminal)
out,terminal=execute(['/usr/bin/python3','-c','import socket; socket.socket(socket.AF_INET6,socket.SOCK_STREAM)'])
assert terminal['code']!=0 and 'Operation not permitted' in out,(out,terminal)

# DNS must be denied before the loopback exception; ordinary loopback traffic must work.
def echo(sock):
    while True:
        data,peer=sock.recvfrom(1024);sock.sendto(data,peer)
for port in [53,5354]:
    sock=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);sock.bind(('127.0.0.1',port))
    threading.Thread(target=echo,args=(sock,),daemon=True).start()
for port,expected in [(53,'blocked'),(5354,'echo')]:
    program=f'''import socket
s=socket.socket(socket.AF_INET,socket.SOCK_DGRAM);s.settimeout(.5)
try:
    s.sendto(b'echo',('127.0.0.1',{port}));print(s.recv(1024).decode())
except (TimeoutError,PermissionError): print('blocked')'''
    out,terminal=execute(['/usr/bin/python3','-c',program]);assert out.strip()==expected and terminal['code']==0,(port,out,terminal)
out,terminal=execute(['/bin/sh','-c','echo $$ > timeout-pid; exec sleep 30'],100)
assert terminal['t']=='error' and terminal['code']=='timeoutExceeded',(out,terminal)
pid=int(open('/sandbox/timeout-pid').read())
for _ in range(20):
    if not os.path.exists('/proc/'+str(pid)): break
    time.sleep(.05)
assert not os.path.exists('/proc/'+str(pid)), 'timed-out command must actually be killed and reaped'
print('privileged supervisor qualification passed')
PY
