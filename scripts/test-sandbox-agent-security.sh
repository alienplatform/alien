#!/usr/bin/env bash
# Destructive cgroup/firewall checks run only in a disposable container namespace.
set -euo pipefail
[ "$#" -eq 1 ] || { echo "usage: $0 <agent-lib-test-binary>" >&2; exit 2; }
binary=$(realpath "$1")
fixture=$(mktemp -d)
container="alien-agent-security-$$"
image="alien-agent-security:$$"
cleanup() {
  result=$?
  trap - EXIT
  docker rm -f "$container" >/dev/null 2>&1 || result=1
  docker image rm "$image" >/dev/null 2>&1 || result=1
  rm -rf "$fixture" || result=1
  exit "$result"
}
# Only register resources after they exist, preserving failures during setup.
trap 'rm -rf "$fixture"' EXIT
cat > "$fixture/Dockerfile" <<'DOCKERFILE'
FROM ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends python3 util-linux iproute2 libgcc-s1 && rm -rf /var/lib/apt/lists/*
DOCKERFILE
docker build -q -t "$image" "$fixture" >/dev/null
trap 'docker image rm "$image" >/dev/null; rm -rf "$fixture"' EXIT
docker create --name "$container" --privileged --cgroupns=private \
  --mount "type=bind,source=$binary,target=/agent-tests,readonly" \
  -e HOST_NET_NS="$(readlink /proc/self/ns/net)" \
  -e HOST_MNT_NS="$(readlink /proc/self/ns/mnt)" \
  -e HOST_CGROUP_NS="$(readlink /proc/self/ns/cgroup)" \
  "$image" bash -euo pipefail -c '
    test "$(readlink /proc/self/ns/net)" != "$HOST_NET_NS"
    test "$(readlink /proc/self/ns/mnt)" != "$HOST_MNT_NS"
    test "$(readlink /proc/self/ns/cgroup)" != "$HOST_CGROUP_NS"
    test "$(stat -fc %T /sys/fs/cgroup)" = cgroup2fs
    for name in \
      privilege::tests::root_image_capabilities_survive_exec_without_network_administration \
      privilege::tests::root_image_does_not_inherit_supervisor_network_administration \
      privilege::tests::cgroup_attribution_filters_bpf_without_disabling_ipv6 \
      gateway_egress::guest_network_tests::device_allowlist_denies_raw_commands_and_preserves_terminals \
      image_command::tests::saved_image_user_controls_boot_without_changing_exec_identity \
      image_command::tests::legacy_egress_refuses_root_image_before_spawn \
      peer::tests::socket_mark_separates_provider_from_every_guest_uid \
      protected_files::tests::root_image_cannot_replace_bootstrap_files \
      gateway_egress::guest_network_tests::root_and_child_uids_keep_redirects_and_cannot_escape
    do
      /agent-tests --list --ignored --exact "$name" | grep -Fx "$name: test"
      unshare --mount /agent-tests --ignored --exact "$name" --nocapture
      if test "$name" = privilege::tests::cgroup_attribution_filters_bpf_without_disabling_ipv6 ||
         test "$name" = gateway_egress::guest_network_tests::device_allowlist_denies_raw_commands_and_preserves_terminals; then
        rmdir /sys/fs/cgroup/alien-guest
      fi
    done
  ' >/dev/null
trap cleanup EXIT
docker start -a "$container"
test "$(docker inspect --format '{{.State.ExitCode}}' "$container")" = 0
