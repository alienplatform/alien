#!/usr/bin/env bash
# The real agent with image permissions, booted as an AWS sandbox with `none` egress in a
# disposable privileged container without a network: the provider's run hook answers 200, and
# when the image cannot start safely it never starts while the agent stays up to say why.
set -euo pipefail
[ "$#" -eq 1 ] || { echo "usage: $0 <agent-binary>" >&2; exit 2; }
agent=$(realpath "$1")
fixture=$(mktemp -d)
image="alien-agent-boot-test:$$"
trap 'docker image rm "$image" >/dev/null 2>&1 || true; rm -rf "$fixture"' EXIT
cat > "$fixture/Dockerfile" <<'DOCKERFILE'
FROM ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends curl python3 util-linux iproute2 procps && rm -rf /var/lib/apt/lists/*
DOCKERFILE
cat > "$fixture/boot.sh" <<'BOOT'
set -euo pipefail
variant=$1
mkdir -p /session /opt/alien
# The image's command; its argument marks it in the process list.
printf '%s\n' '{"user":"0","workingDirectory":"/","command":["/bin/sleep","3601"],"env":{"PATH":"/usr/bin:/bin"}}' \
  > /opt/alien/image-command.json
echo 'app:x:1000:1000::/:/bin/sh' >> /etc/passwd
echo 'app:x:1000:' >> /etc/group
export ALIEN_SANDBOX_ROOT=/session ALIEN_SANDBOX_PORT=8080 ALIEN_SANDBOX_AUTHORIZATION=transport \
  ALIEN_SANDBOX_EXEC_UID=1000 ALIEN_SANDBOX_EXEC_GID=1000 ALIEN_SANDBOX_ISOLATION=uid-split \
  ALIEN_SANDBOX_IMAGE_COMMAND=image-permissions-v1 ALIEN_SANDBOX_EGRESS_RUN_HOOK=true \
  ALIEN_SANDBOX_EGRESS='"none"' ALIEN_SANDBOX_IMAGE_PERMISSIONS_V=1 \
  ALIEN_SANDBOX_IMAGE_PERMISSIONS='{"mounts":[{"type":"nfs","path":"/workspace"},{"type":"tmpfs","path":"/var/lib/app/scratch","sizeBytes":1048576}],"connections":[{"cidr":"10.0.1.5/32","port":2049}],"onPause":["/bin/true"],"onResume":["/bin/true"]}'
ip link set lo up
start=(/agent)
case "$variant" in
  started) ;;
  # The seal needs SYS_MODULE: an agent started without it must not start the image.
  seal-fails) start=(setpriv --bounding-set -sys_module /agent) ;;
  # A step of the run hook itself fails: something answers at 1.1.1.1:443 without the gateway,
  # so the egress check refuses.
  run-hook-step-fails)
    ip addr add 1.1.1.1/32 dev lo
    python3 -c '
import socket
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("1.1.1.1", 443)); s.listen(16)
while True:
    c, _ = s.accept(); c.recv(1024); c.sendall(b"HTTP/1.0 200 OK\r\n\r\n"); c.close()
' &
    ;;
esac
"${start[@]}" > /tmp/agent.log 2>&1 &
for _ in $(seq 100); do curl -s -o /dev/null http://127.0.0.1:8080/v1/health && break; sleep 0.1; done
as_provider() { setpriv --reuid 991 --regid 991 --clear-groups "$@"; }
code=$(as_provider curl -s -o /tmp/hook-body -w '%{http_code}' -X POST -d '{}' \
  http://127.0.0.1:8080/aws/lambda-microvms/runtime/v1/run)
echo "run hook: $code $(cat /tmp/hook-body)"
sleep 2
exec_body=$(as_provider curl -s -X POST -H 'content-type: application/json' \
  -d '{"command":["/bin/echo","exec-ran"],"timeoutMs":5000}' http://127.0.0.1:8080/v1/exec || true)
echo "exec: ${exec_body:0:400}"
fail() { echo "FAIL ($variant): $*"; echo '--- agent log'; tail -40 /tmp/agent.log; exit 1; }
[ "$code" = 200 ] || fail "the run hook answered $code"
if [ "$variant" = started ]; then
  pgrep -f '^/bin/sleep 3601$' >/dev/null || fail "the image did not start"
  [ ! -e /run/alien/boot-error ] || fail "boot error without a failure: $(cat /run/alien/boot-error)"
  grep -q '"data":"ZXhlYy1yYW4K"' <<<"$exec_body" || fail "exec did not run"
  grep -q 'mount broker: sealed:.*created: /var/lib/app/scratch' /tmp/agent.log \
    || fail "the seal did not create the missing declared path"
  [ -d /var/lib/app/scratch ] || fail "/var/lib/app/scratch missing"
  echo "OK started: run hook 200, image running, exec ran, declared path created before the seal"
else
  ! pgrep -f '^/bin/sleep 3601$' >/dev/null || fail "the image started"
  [ -r /run/alien/boot-error ] || fail "no /run/alien/boot-error"
  reason=$(cat /run/alien/boot-error)
  [ "$(stat -c %a /run/alien/boot-error)" = 644 ] || fail "boot-error mode"
  grep -qF "the image did not start (/run/alien/boot-error)" <<<"$exec_body" || fail "exec did not answer the boot error"
  ! grep -q 'ZXhlYy1yYW4K' <<<"$exec_body" || fail "exec ran"
  grep -qF 'the image will not start' /tmp/agent.log || fail "the reason is not on the agent's stderr"
  case "$variant" in
    seal-fails) grep -q 'SYS_MODULE' <<<"$reason" || fail "reason: $reason" ;;
    run-hook-step-fails) grep -q '1.1.1.1:443 is reachable without the gateway' <<<"$reason" || fail "reason: $reason" ;;
  esac
  echo "OK $variant: run hook 200, image never started, exec answers: $reason"
fi
BOOT
docker build -q -t "$image" "$fixture" >/dev/null
for variant in started seal-fails run-hook-step-fails; do
  docker run --rm --privileged --cgroupns=private --network none \
    --mount "type=bind,source=$agent,target=/agent,readonly" \
    --mount "type=bind,source=$fixture/boot.sh,target=/boot.sh,readonly" \
    "$image" bash /boot.sh "$variant"
done
