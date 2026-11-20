#!/usr/bin/env bash
# The mount broker against a real NFS server, in a disposable privileged container.
# Needs the host's nfsv4 client module (`modprobe nfsv4`). The server is the kernel's nfsd when it
# serves NFSv4 (`modprobe nfsd`), else nfs-ganesha in user space (some CI kernels build nfsd
# without NFSv4). ALIEN_BROKER_NFS_SERVER=knfsd|ganesha picks one.
set -euo pipefail
[ "$#" -eq 2 ] || { echo "usage: $0 <agent-lib-test-binary> <agent-binary>" >&2; exit 2; }
tests=$(realpath "$1")
agent=$(realpath "$2")
fixture=$(mktemp -d)
image="alien-mount-broker-test:$$"
trap 'docker image rm "$image" >/dev/null 2>&1 || true; rm -rf "$fixture"' EXIT
cat > "$fixture/Dockerfile" <<'DOCKERFILE'
FROM ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends python3 util-linux iproute2 libgcc-s1 nfs-kernel-server nfs-ganesha nfs-ganesha-vfs nfs-common rpcbind procps && rm -rf /var/lib/apt/lists/*
DOCKERFILE
docker build -q -t "$image" "$fixture" >/dev/null
name=mount_broker::integration_tests::broker_allows_declared_mounts_and_refuses_everything_else
# Twice: as on a kernel with module loading, then as on one without (no kernel.modprobe).
for variant in modules no-modules; do
rounds=${ALIEN_BROKER_RACE_ROUNDS:-40}
extra=()
if [ "$variant" = no-modules ]; then rounds=5; extra=(-e ALIEN_BROKER_NO_MODPROBE=1); fi
echo "mount broker integration: $variant"
docker run --rm --privileged --cgroupns=private \
  -e "ALIEN_BROKER_RACE_ROUNDS=$rounds" -e "ALIEN_BROKER_NFS_SERVER=${ALIEN_BROKER_NFS_SERVER:-}" "${extra[@]}" \
  --mount "type=bind,source=$tests,target=/agent-tests,readonly" \
  --mount "type=bind,source=$agent,target=/agent,readonly" \
  "$image" bash -euo pipefail -c '
    test "$(stat -fc %T /sys/fs/cgroup)" = cgroup2fs
    mkdir -p /export && mount -t tmpfs tmpfs /export && chmod 0777 /export && echo hello > /export/hello.txt
    # nfsd registers its listeners with rpcbind on some kernels (CI): without it rpc.nfsd fails
    # with "writing fd to kernel failed: errno 111".
    rpcbind -w
    mount -t nfsd nfsd /proc/fs/nfsd 2>/dev/null || true
    echo "kernel $(uname -r), nfsd versions: $(cat /proc/fs/nfsd/versions 2>/dev/null || echo none)"
    server=${ALIEN_BROKER_NFS_SERVER:-}
    if [ -z "$server" ]; then
      if grep -qE "(^| )[+-]4( |$)" /proc/fs/nfsd/versions 2>/dev/null; then server=knfsd; else server=ganesha; fi
    fi
    echo "test NFS server: $server"
    if [ "$server" = ganesha ]; then
      cat > /etc/ganesha/ganesha.conf <<GANESHA
NFS_CORE_PARAM { Protocols = 4; NFS_Port = 2049; Bind_addr = 127.0.0.1; Enable_NLM = false; Enable_RQUOTA = false; }
NFSV4 { Minor_Versions = 1, 2; Grace_Period = 0; Lease_Lifetime = 10; Graceless = true; }
EXPORT { Export_Id = 1; Path = /export; Pseudo = /; Protocols = 4; Transports = TCP; Access_Type = RW;
  Squash = No_Root_Squash; SecType = sys; FSAL { Name = VFS; } }
LOG { Default_Log_Level = WARN; }
GANESHA
      ganesha.nfsd -L /tmp/ganesha.log -f /etc/ganesha/ganesha.conf -p /tmp/ganesha.pid
      for _ in $(seq 50); do ss -Hltn "sport = :2049" | grep -q . && break; sleep 0.2; done
      ss -Hltn "sport = :2049" | grep -q . || { echo "nfs-ganesha did not listen on 2049" >&2; cat /tmp/ganesha.log >&2; exit 1; }
    else
    echo "/export 127.0.0.1(rw,sync,no_subtree_check,no_root_squash,fsid=0,insecure)" > /etc/exports
    exportfs -ra
    rpc.mountd -N 2 -N 3
    # Kernels from 6.11 keep the served versions per network namespace, and their default in a
    # new one differs (6.12: NFSv3 only). Set them before the nfsd threads start, one token at a
    # time so a version the kernel lacks is named, and give rpc.nfsd the same set.
    grep -qE "(^| )[+-]4( |$)" /proc/fs/nfsd/versions || { echo "the host kernel nfsd has no NFSv4 (CONFIG_NFSD_V4)" >&2; exit 1; }
    for v in +4 +4.1 +4.2 -3; do
      echo "$v" > /proc/fs/nfsd/versions || { echo "the kernel refused nfsd version $v" >&2; exit 1; }
    done
    rpc.nfsd -N 3 -V 4 -V 4.1 -V 4.2 2
    echo "nfsd versions: $(cat /proc/fs/nfsd/versions) (kernel $(uname -r))"
    grep -qw "+4.2" /proc/fs/nfsd/versions || { echo "the test NFS server does not serve NFSv4.2" >&2; exit 1; }
    fi
    # The kernel client against the server directly: a failure here is the fixture, not the broker.
    mkdir -p /tmp/nfs-probe
    if ! mount -t nfs4 -o vers=4.2,soft,timeo=20,retrans=1 127.0.0.1:/ /tmp/nfs-probe; then
      echo "the kernel NFS client cannot mount the test server directly" >&2
      dmesg 2>/dev/null | tail -20 >&2 || true
      exit 1
    fi
    test "$(cat /tmp/nfs-probe/hello.txt)" = hello || { echo "the test export has no hello.txt" >&2; exit 1; }
    umount /tmp/nfs-probe
    /agent-tests --list --ignored --exact "'"$name"'" | grep -Fx "'"$name"': test"
    unshare --mount --propagation private /agent-tests --ignored --exact "'"$name"'" --nocapture --test-threads=1
  '
done
# The whole agent, booted as an AWS sandbox: the run hook's answer and the boot error.
"$(dirname "$0")/test-sandbox-agent-boot.sh" "$agent"
