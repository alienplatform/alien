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
    # The previous test left its cgroups (with the child its escape attempt made) and, in this
    # network namespace, its firewall behind: this one gets a network namespace of its own.
    find /sys/fs/cgroup/alien-guest -depth -type d -exec rmdir {} +
    # Each in a network namespace of its own (the second: the IPv6 policy rules alone, as on a
    # kernel without IPv6 netfilter).
    for name in \
      gateway_egress::guest_network_tests::image_nfs_leaves_only_through_its_mark_and_declared_connections \
      gateway_egress::guest_network_tests::without_ip6_netfilter_policy_rules_refuse_ipv6_nfs_ports
    do
    /agent-tests --list --ignored --exact "$name" | grep -Fx "$name: test"
    # The namespace file an earlier run left (its mount went with its mount namespace).
    rm -f /run/netns/remote
    unshare --mount --net bash -euo pipefail -c "
      ip link set lo up
      for knob in all default lo; do echo 0 > /proc/sys/net/ipv6/conf/\$knob/disable_ipv6; done
      # Off-VM servers: another network namespace behind a veth pair.
      ip netns add remote
      ip link add veth0 type veth peer name veth1 netns remote
      ip addr add 192.168.77.1/24 dev veth0
      ip -6 addr add fd01::1/64 dev veth0 nodad
      ip link set veth0 up
      for knob in all default lo; do
        echo 0 | ip netns exec remote tee /proc/sys/net/ipv6/conf/\$knob/disable_ipv6 >/dev/null
      done
      ip -n remote link set lo up
      ip -n remote addr add 10.0.1.5/32 dev lo
      ip -n remote addr add 10.0.2.5/32 dev lo
      ip -n remote -6 addr add fd00::5/128 dev lo nodad
      ip -n remote addr add 192.168.77.2/24 dev veth1
      ip -n remote -6 addr add fd01::2/64 dev veth1 nodad
      ip -n remote link set veth1 up
      ip route add 10.0.0.0/16 via 192.168.77.2
      ip -6 route add fd00::/64 via fd01::2
      export ALIEN_TEST_REMOTE_NETNS=/run/netns/remote
      exec /agent-tests --ignored --exact $name --nocapture
    "
    done
  ' >/dev/null
trap cleanup EXIT
docker start -a "$container"
test "$(docker inspect --format '{{.State.ExitCode}}' "$container")" = 0
