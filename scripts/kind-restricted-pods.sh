#!/usr/bin/env bash
# Proves two Operator defaults on a real cluster:
#
#   1. Restricted by default: the Deployment and DaemonSet the Kubernetes controllers render
#      for a workload that declares no `security` are admitted and run in a namespace that
#      enforces the Pod Security `restricted` profile. An image that runs as root is refused
#      by the kubelet, so the controller can name the opt-out.
#   2. One mirror: with a site registry configured (`global.imageRegistry`, the Operator's
#      IMAGE_REGISTRY) every pod runs an image pulled from that registry by digest. The
#      release names an image on a host that does not exist; only the mirror has it.
#
# Needs docker, kind, kubectl and cargo. Creates and deletes its own Kind cluster and a local
# registry container. Set ALIEN_KIND_KEEP=1 to keep them.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cluster="${ALIEN_KIND_CLUSTER:-alien-restricted-pods}"
registry_name="alien-kind-mirror"
registry_port=5001
mirror="localhost:${registry_port}"
work="$(mktemp -d)"

cleanup() {
  if [ "${ALIEN_KIND_KEEP:-}" != "1" ]; then
    kind delete cluster --name "$cluster" >/dev/null 2>&1 || true
    docker rm -f "$registry_name" >/dev/null 2>&1 || true
  fi
  rm -rf "$work"
}
trap cleanup EXIT

# --- local registry: the site mirror ----------------------------------------------------------
docker rm -f "$registry_name" >/dev/null 2>&1 || true
docker run -d --restart=always -p "127.0.0.1:${registry_port}:5000" --name "$registry_name" registry:2 >/dev/null

cat >"$work/kind.yaml" <<'EOF'
kind: Cluster
apiVersion: kind.x-k8s.io/v1alpha4
containerdConfigPatches:
  - |-
    [plugins."io.containerd.grpc.v1.cri".registry]
      config_path = "/etc/containerd/certs.d"
EOF
kind delete cluster --name "$cluster" >/dev/null 2>&1 || true
# Keep this cluster out of the caller's kubeconfig: kind would otherwise switch its current-context.
export KUBECONFIG="$work/kubeconfig"
kind create cluster --name "$cluster" --config "$work/kind.yaml" --wait 120s --kubeconfig "$KUBECONFIG"
[ "$(kubectl config current-context)" = "kind-${cluster}" ]

# Nodes reach the mirror as localhost:5001 through the registry container.
docker network connect kind "$registry_name" 2>/dev/null || true
for node in $(kind get nodes --name "$cluster"); do
  docker exec "$node" mkdir -p "/etc/containerd/certs.d/${mirror}"
  docker exec -i "$node" sh -c "cat > /etc/containerd/certs.d/${mirror}/hosts.toml" <<EOF
[host."http://${registry_name}:5000"]
EOF
done

# --- two images in the mirror: one that runs as a numeric non-root user, one as root ----------
cat >"$work/Dockerfile.nonroot" <<'EOF'
FROM busybox:1.36
USER 65532:65532
CMD ["sleep", "3600"]
EOF
cat >"$work/Dockerfile.root" <<'EOF'
FROM busybox:1.36
CMD ["sleep", "3600"]
EOF
push() { # <dockerfile> <repository> -> prints the digest
  docker build -q -f "$1" -t "${mirror}/$2:v1" "$work" >/dev/null
  docker push "${mirror}/$2:v1" 2>&1 | sed -n 's/.*digest: \(sha256:[a-f0-9]*\).*/\1/p' | tail -1
}
nonroot_digest="$(push "$work/Dockerfile.nonroot" test/nonroot)"
root_digest="$(push "$work/Dockerfile.root" test/root)"
echo "mirror holds test/nonroot@${nonroot_digest} and test/root@${root_digest}"

# --- the manifests the Operator's controllers render --------------------------------------------
render() { # <image> <dir>
  mkdir -p "$2"
  (cd "$root" && ALIEN_KIND_MANIFEST_DIR="$2" ALIEN_KIND_IMAGE="$1" ALIEN_KIND_IMAGE_REGISTRY="$mirror" \
    cargo test -p alien-infra --features all-platforms,test-utils --lib for_the_kind_test -- --ignored >/dev/null)
}
# The release names a host that does not exist, so a pod can only run from the mirror.
render "registry.invalid/test/nonroot@${nonroot_digest}" "$work/nonroot"
render "registry.invalid/test/root@${root_digest}" "$work/root"

failures=()

enforce() { # <namespace>
  kubectl create namespace "$1"
  kubectl label namespace "$1" pod-security.kubernetes.io/enforce=restricted \
    pod-security.kubernetes.io/enforce-version=latest
}

# --- 1. restricted by default -------------------------------------------------------------------
# The workloads declare no `security`; the namespace enforces `restricted`.
enforce restricted-ok
kubectl -n restricted-ok apply -f "$work/nonroot/container.yaml" -f "$work/nonroot/daemon.yaml"
if ! kubectl -n restricted-ok rollout status deployment/web --timeout=90s \
  || ! kubectl -n restricted-ok rollout status daemonset/agent --timeout=90s; then
  refusal="$(kubectl -n restricted-ok get rs,ds -o json | python3 -c '
import json, sys
for item in json.load(sys.stdin)["items"]:
    for cond in item.get("status", {}).get("conditions", []):
        if "violates PodSecurity" in cond.get("message", ""):
            print(item["kind"] + "/" + item["metadata"]["name"] + ": " + cond["message"])
' | head -2)"
  failures+=("restricted: the namespace did not admit the default pods. ${refusal:-no PodSecurity message found}")
fi

# --- 2. one mirror, by digest -------------------------------------------------------------------
# A namespace that admits anything, so only the image reference is under test. The release names
# a host that does not exist; with the Operator's site registry set, every pod must name the mirror.
kubectl create namespace mirror
kubectl -n mirror apply -f "$work/nonroot/container.yaml" -f "$work/nonroot/daemon.yaml"
sleep 20
images="$(kubectl -n mirror get pods -o jsonpath='{range .items[*]}{.spec.containers[*].image}{"\n"}{end}')"
echo "pod images: $images"
if [ -z "$images" ] || echo "$images" | grep -qv '^localhost:5001/test/nonroot@sha256:'; then
  failures+=("mirror: pod images are not the mirror by digest: $(echo "$images" | tr '\n' ' ')")
fi

if [ "${#failures[@]}" -gt 0 ]; then
  printf 'FAILED %s\n' "${failures[@]}" >&2
  exit 1
fi
echo "restricted pods and mirror: ok"
