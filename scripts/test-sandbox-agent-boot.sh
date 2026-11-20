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
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends curl python3 util-linux iproute2 procps keyutils && rm -rf /var/lib/apt/lists/*
DOCKERFILE
cat > "$fixture/boot.sh" <<'BOOT'
set -euo pipefail
variant=$1
# A fresh PID namespace, as the agent has where image permissions are accepted: no kthreadd at
# PID 2 (the first child, gone by the time the agent runs). The `initial-pid-namespace` variant
# puts a kthreadd stand-in there, as an agent in the kernel's initial PID namespace would see.
if [ -z "${BOOT_FRESH_PID_NAMESPACE:-}" ]; then
  exec env BOOT_FRESH_PID_NAMESPACE=1 unshare --fork --pid --mount-proc bash "$0" "$@"
fi
if [ "$variant" = initial-pid-namespace ]; then
  sleep infinity &
  [ "$!" = 2 ] || { echo "FAIL: the kthreadd stand-in is PID $!, not 2"; exit 1; }
  echo kthreadd > /tmp/kthreadd-comm
  mount --bind /tmp/kthreadd-comm /proc/2/comm
fi
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
  ALIEN_SANDBOX_IMAGE_PERMISSIONS='{"mounts":[{"type":"nfs","path":"/workspace"},{"type":"tmpfs","path":"/var/lib/app/scratch","sizeBytes":1048576}],"connections":[{"cidr":"10.0.1.5/32","port":2049}],"onPause":["/bin/sh","-c","echo paused >> /tmp/steps"],"onResume":["/bin/sh","-c","echo resumed >> /tmp/steps"]}'
ip link set lo up
# The seal writes kernel-wide helper settings that can only be lowered until reboot. Files bound
# over them in this container's mount namespace (inherited by the agent and its broker) take the
# writes instead, so the host keeps its own values.
mkdir -p /tmp/kernel-settings
for setting in usermodehelper/bset usermodehelper/inheritable modprobe; do
  [ -e "/proc/sys/kernel/$setting" ] || continue
  standin=/tmp/kernel-settings/${setting//\//-}
  # Unsealed values, so the check below sees the seal's write even on a host sealed before.
  case "$setting" in modprobe) echo /sbin/modprobe ;; *) printf '4294967295\t31\n' ;; esac > "$standin"
  mount --bind "$standin" "/proc/sys/kernel/$setting"
done
start=(/agent)
case "$variant" in
  started) ;;
  # The seal needs SYS_MODULE: an agent started without it must not start the image.
  seal-fails) start=(setpriv --bounding-set -sys_module /agent) ;;
  initial-pid-namespace) ;;
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
  image_pid=$(pgrep -f '^/bin/sleep 3601$') || fail "the image did not start"
  # The image's mount namespace: kernel settings are read-only there, and only there.
  for point in /proc/sys /proc/sysrq-trigger /sys; do
    awk -v p="$point" '$5 == p { o = $6 } END { exit !(o ~ /^ro,/) }' "/proc/$image_pid/mountinfo" \
      || fail "$point is not read-only in the image's mount namespace"
  done
  ! nsenter -t "$image_pid" -m sh -c 'exec 3>>/proc/sys/kernel/core_pattern' 2>/dev/null \
    || fail "/proc/sys/kernel/core_pattern opens for writing in the image's mount namespace"
  if [ -e /sys/kernel/uevent_helper ]; then
    ! nsenter -t "$image_pid" -m setpriv --bounding-set -all --inh-caps -all sh -c 'exec 3>>/sys/kernel/uevent_helper' 2>/dev/null \
      || fail "/sys/kernel/uevent_helper opens for writing in the image's mount namespace"
  fi
  [ ! -e /run/alien/boot-error ] || fail "boot error without a failure: $(cat /run/alien/boot-error)"
  grep -q '"data":"ZXhlYy1yYW4K"' <<<"$exec_body" || fail "exec did not run"
  [ ! -d /proc/2 ] || [ "$(cat /proc/2/comm)" != kthreadd ] || fail "PID 2 is kthreadd"
  [ "$(stat -c %a /run/alien/boot-report)" = 644 ] || fail "boot-report mode"
  for fact in '^ip6 nftables: installed$' '^ipv6 addresses (/proc/net/if_inet6): ' \
    '^ipv6 default route (/proc/net/ipv6_route): ' '^seal: usermodehelper bset .*modprobe '; do
    grep -q "$fact" /run/alien/boot-report || fail "boot report lacks $fact: $(cat /run/alien/boot-report)"
  done
  grep -q 'boot report' /tmp/agent.log || fail "the boot report is not in the log"
  # The run hook installs the firewall: its counters are read from then on, never before.
  ! grep -q ' error: ' /run/alien/unmarked-drops || fail "counters: $(cat /run/alien/unmarked-drops)"
  grep -q '^[0-9]* boot-run-hook ip jumps=\[[0-9, ]*\] loopback_return=[0-9]* dropped=[0-9]* ip6 ' \
    /run/alien/unmarked-drops || fail "counters: $(cat /run/alien/unmarked-drops)"
  [ "$(cat /tmp/kernel-settings/usermodehelper-bset)" = "$(printf '0\t0')" ] \
    || fail "the seal did not write the bset stand-in"
  [ "$(cat /tmp/kernel-settings/usermodehelper-inheritable)" = "$(printf '0\t0')" ] \
    || fail "the seal did not write the inheritable stand-in"
  [ ! -e /proc/sys/kernel/modprobe ] || [ -z "$(cat /tmp/kernel-settings/modprobe)" ] \
    || fail "the seal did not write the modprobe stand-in"
  # The provider's suspend and resume: each answers 200 at once, runs the image's step and
  # leaves its timeline in the lifecycle report.
  for hook in suspend resume; do
    t0=$(date +%s%N)
    code=$(as_provider curl -s -o /tmp/hook-body -w '%{http_code}' -X POST -d '{}' \
      "http://127.0.0.1:8080/aws/lambda-microvms/runtime/v1/$hook")
    took=$(( ($(date +%s%N) - t0) / 1000000 ))
    echo "$hook hook: $code in $took ms"
    [ "$code" = 200 ] || fail "the $hook hook answered $code: $(cat /tmp/hook-body)"
    [ "$took" -lt 5000 ] || fail "the $hook hook took $took ms"
  done
  [ "$(cat /tmp/steps)" = "$(printf 'paused\nresumed')" ] || fail "steps: $(cat /tmp/steps)"
  for fact in 'suspend hook: received' 'onPause: step succeeded' 'suspend hook: 200' \
    'resume hook: network check passed' 'onResume: step succeeded' 'resume hook: 200'; do
    grep -q "$fact" /run/alien/lifecycle-report || fail "lifecycle report lacks $fact: $(cat /run/alien/lifecycle-report)"
  done
  [ "$(stat -c %a /run/alien/lifecycle-report)" = 644 ] || fail "lifecycle-report mode"
  echo "lifecycle report:"; sed 's/^/  /' /run/alien/lifecycle-report
  echo "boot report:"; sed 's/^/  /' /run/alien/boot-report
  echo "OK started: run hook 200, image running, exec ran, sealed"
else
  ! pgrep -f '^/bin/sleep 3601$' >/dev/null || fail "the image started"
  [ -r /run/alien/boot-error ] || fail "no /run/alien/boot-error"
  reason=$(cat /run/alien/boot-error)
  [ "$(stat -c %a /run/alien/boot-error)" = 644 ] || fail "boot-error mode"
  grep -qF "the image did not start (/run/alien/boot-error)" <<<"$exec_body" || fail "exec did not answer the boot error"
  ! grep -q 'ZXhlYy1yYW4K' <<<"$exec_body" || fail "exec ran"
  grep -qF 'the image will not start' /tmp/agent.log || fail "the reason is not on the agent's stderr"
  case "$variant" in
    seal-fails) grep -q 'SYS_MODULE' <<<"$reason" || fail "reason: $reason"
      grep -q '^seal: failed: .*SYS_MODULE' /run/alien/boot-report || fail "boot report: $(cat /run/alien/boot-report)" ;;
    run-hook-step-fails) grep -q '1.1.1.1:443 is reachable without the gateway' <<<"$reason" || fail "reason: $reason" ;;
    initial-pid-namespace) grep -q 'is kthreadd: the agent runs in the kernel.s initial PID namespace' <<<"$reason" || fail "reason: $reason"
      grep -q '^seal: failed: .*own namespaces' /run/alien/boot-report || fail "boot report: $(cat /run/alien/boot-report)" ;;
  esac
  echo "OK $variant: run hook 200, image never started, exec answers: $reason"
fi
BOOT
docker build -q -t "$image" "$fixture" >/dev/null
host_settings() { cat /proc/sys/kernel/usermodehelper/bset /proc/sys/kernel/usermodehelper/inheritable /proc/sys/kernel/modprobe 2>/dev/null || true; }
before=$(host_settings)
for variant in started seal-fails run-hook-step-fails initial-pid-namespace; do
  docker run --rm --privileged --cgroupns=private --network none \
    --mount "type=bind,source=$agent,target=/agent,readonly" \
    --mount "type=bind,source=$fixture/boot.sh,target=/boot.sh,readonly" \
    "$image" bash /boot.sh "$variant"
done
[ "$(host_settings)" = "$before" ] || { echo "FAIL: the test changed the host's helper settings" >&2; exit 1; }
echo "OK the host's helper settings are unchanged"
