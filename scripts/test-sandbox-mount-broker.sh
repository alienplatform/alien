#!/usr/bin/env bash
# The mount broker against a real kernel NFS server, in a disposable privileged container.
# Needs the host's nfsd and nfsv4 modules (`modprobe nfsd nfsv4`).
set -euo pipefail
[ "$#" -eq 2 ] || { echo "usage: $0 <agent-lib-test-binary> <agent-binary>" >&2; exit 2; }
tests=$(realpath "$1")
agent=$(realpath "$2")
fixture=$(mktemp -d)
image="alien-mount-broker-test:$$"
trap 'docker image rm "$image" >/dev/null 2>&1 || true; rm -rf "$fixture"' EXIT
cat > "$fixture/Dockerfile" <<'DOCKERFILE'
FROM ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends python3 util-linux iproute2 libgcc-s1 nfs-kernel-server nfs-common rpcbind procps && rm -rf /var/lib/apt/lists/*
DOCKERFILE
docker build -q -t "$image" "$fixture" >/dev/null
name=mount_broker::integration_tests::broker_allows_declared_mounts_and_refuses_everything_else
# Twice: as on a kernel with module loading, then as on one without (no kernel.modprobe).
for variant in modules no-modules; do
rounds=${ALIEN_BROKER_RACE_ROUNDS:-40}
extra=()
# The second run also puts the image's / on ramfs: no immutable attribute there.
if [ "$variant" = no-modules ]; then rounds=5; extra=(-e ALIEN_BROKER_NO_MODPROBE=1 -e ALIEN_BROKER_ROOT_RAMFS=1); fi
echo "mount broker integration: $variant"
docker run --rm --privileged --cgroupns=private \
  -e "ALIEN_BROKER_RACE_ROUNDS=$rounds" "${extra[@]}" \
  --mount "type=bind,source=$tests,target=/agent-tests,readonly" \
  --mount "type=bind,source=$agent,target=/agent,readonly" \
  "$image" bash -euo pipefail -c '
    test "$(stat -fc %T /sys/fs/cgroup)" = cgroup2fs
    mkdir -p /export && mount -t tmpfs tmpfs /export && chmod 0777 /export && echo hello > /export/hello.txt
    mount -t nfsd nfsd /proc/fs/nfsd
    echo "/export 127.0.0.1(rw,sync,no_subtree_check,no_root_squash,fsid=0,insecure)" > /etc/exports
    exportfs -ra
    # nfsd registers its listeners with rpcbind on some kernels (CI): without it rpc.nfsd fails
    # with "writing fd to kernel failed: errno 111".
    rpcbind -w
    rpc.mountd -N 2 -N 3
    rpc.nfsd -N 3 -V 4.1 -V 4.2 2
    /agent-tests --list --ignored --exact "'"$name"'" | grep -Fx "'"$name"': test"
    unshare --mount --propagation private /agent-tests --ignored --exact "'"$name"'" --nocapture --test-threads=1
  '
done
