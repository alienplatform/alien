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

enforce() { # <namespace>
  kubectl create namespace "$1"
  kubectl label namespace "$1" pod-security.kubernetes.io/enforce=restricted \
    pod-security.kubernetes.io/enforce-version=latest
}

# --- 1. restricted by default, from the mirror, by digest ---------------------------------------
enforce restricted-ok
kubectl -n restricted-ok apply -f "$work/nonroot/container.yaml" -f "$work/nonroot/daemon.yaml"
kubectl -n restricted-ok rollout status deployment/web --timeout=180s
kubectl -n restricted-ok rollout status daemonset/agent --timeout=180s

pods="$(kubectl -n restricted-ok get pods -o json)"
count="$(echo "$pods" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["items"]))')"
[ "$count" -ge 2 ]
echo "$pods" | python3 -c '
import json, sys
pods = json.load(sys.stdin)["items"]
for pod in pods:
    name = pod["metadata"]["name"]
    assert pod["status"]["phase"] == "Running", (name, pod["status"]["phase"])
    for status in pod["status"]["containerStatuses"]:
        assert status["imageID"].startswith("localhost:5001/test/nonroot@sha256:"), (name, status["imageID"])
    for container in pod["spec"]["containers"]:
        assert container["image"].startswith("localhost:5001/test/nonroot@sha256:"), (name, container["image"])
print("every pod runs from the mirror by digest:", [p["metadata"]["name"] for p in pods])
'

# --- fail-first: an image that runs as root is refused, with a reason the controller reads ------
enforce restricted-root
kubectl -n restricted-root apply -f "$work/root/container.yaml"
reason=""
for _ in $(seq 1 60); do
  reason="$(kubectl -n restricted-root get pods -o jsonpath='{.items[0].status.containerStatuses[0].state.waiting.message}' 2>/dev/null || true)"
  case "$reason" in *runAsNonRoot*) break ;; esac
  sleep 2
done
case "$reason" in
  *runAsNonRoot*) echo "root image refused: $reason" ;;
  *) echo "expected the kubelet to refuse the root image, got: '$reason'" >&2; exit 1 ;;
esac

# --- 3. every example stack that targets Kubernetes ---------------------------------------------
# Each container and daemon an example declares is rendered by the Operator's controllers from
# the example's own stack (security, ports, environment, persistent storage) and applied to its
# own namespace that enforces `restricted`. Images:
#   - `image` code (the examples' postgres and redis): the real image, copied into the mirror,
#     so the opt-out's user and writable paths are proven against the real image;
#   - source code: stand-in for the image `alien build` produces, which runs as 65532:65532 on
#     Kubernetes (see `kubernetes_image_user`); the stand-in is the non-root test image.
# Links to storage, queues and the like are dropped: their bindings need a cloud, and they do not
# change the pod's security context.
# Every pod must reach Running; readiness is not asserted because the stand-ins do not serve.
bun_bin="${ALIEN_BUN:-bun}"
sdk="$work/sdk"
mkdir -p "$sdk/node_modules/@alienplatform/core"
printf '{ "name": "@alienplatform/core", "type": "module", "exports": "./index.ts" }\n' \
  >"$sdk/node_modules/@alienplatform/core/package.json"
printf 'export * from "%s/packages/core/src/index.ts"\n' "$root" >"$sdk/node_modules/@alienplatform/core/index.ts"

kube_images=()
for example in "$root"/examples/*/; do
  name="$(basename "$example")"
  [ -f "$example/alien.ts" ] || continue
  cp "$example/alien.ts" "$sdk/$name.ts"
  (cd "$sdk" && "$bun_bin" -e "const c = await import('./$name.ts'); console.log(JSON.stringify(c.default))") \
    >"$work/$name.stack.json" 2>"$work/$name.stack.err" || {
      # An example that does not evaluate would go unchecked, so it fails the run.
      echo "example $name: alien.ts does not evaluate: $(tail -1 "$work/$name.stack.err")" >&2; exit 1; }
  # One config file per container or daemon: <example>.<resource>.<kind>.json plus its image.
  python3 - "$work/$name.stack.json" "$work/res" "$name" <<'PY'
import json, os, sys
stack = json.load(open(sys.argv[1]))
out, example = sys.argv[2], sys.argv[3]
os.makedirs(out, exist_ok=True)
for rid, res in stack.get("resources", {}).items():
    config = res["config"]
    kind = config.get("type")
    if kind not in ("container", "daemon"):
        continue
    code = config.get("code") or {}
    image = code.get("image") if code.get("type") == "image" else ""
    path = os.path.join(out, f"{example}.{rid}.{kind}.json")
    config = {k: v for k, v in config.items() if k != "type"}
    # The single Kind node is small; resource sizes do not change the security context.
    if "cpu" in config:
        config["cpu"] = {"min": "0.05", "desired": "0.1"}
    if "memory" in config:
        config["memory"] = {"min": "32Mi", "desired": "128Mi"}
    json.dump({**config, "links": []}, open(path, "w"))
    open(path + ".image", "w").write(image or "")
PY
done

shopt -s nullglob
configs=("$work"/res/*.json)
[ "${#configs[@]}" -gt 0 ] || { echo "no example declares a container or daemon" >&2; exit 1; }
for config in "${configs[@]}"; do
  base="$(basename "$config" .json)"            # <example>.<resource>.<kind>
  kind="${base##*.}"; rest="${base%.*}"; resource="${rest#*.}"; example="${rest%%.*}"
  ns="ex-$(echo "$example-$resource" | tr '[:upper:]_.' '[:lower:]--' | cut -c1-58)"
  source_image="$(cat "$config.image")"
  if [ -n "$source_image" ]; then
    # Copy the real image into the mirror and pin it by digest.
    repo="examples/$(echo "$source_image" | tr ':/' '--')"
    docker pull -q "$source_image" >/dev/null
    docker tag "$source_image" "${mirror}/${repo}:v1"
    digest="$(docker push "${mirror}/${repo}:v1" 2>&1 | sed -n 's/.*digest: \(sha256:[a-f0-9]*\).*/\1/p' | tail -1)"
    image="registry.invalid/${repo}@${digest}"; expect="localhost:5001/${repo}@sha256:"
    kube_images+=("$source_image")
  else
    image="registry.invalid/test/nonroot@${nonroot_digest}"; expect="localhost:5001/test/nonroot@sha256:"
  fi
  dir="$work/ex/$ns"; mkdir -p "$dir"
  if [ "$kind" = container ]; then test_name=write_the_default_deployment_for_the_kind_test; crate_path=container
  else test_name=write_the_default_daemonset_for_the_kind_test; crate_path=daemon; fi
  (cd "$root" && ALIEN_KIND_MANIFEST_DIR="$dir" ALIEN_KIND_IMAGE="$image" ALIEN_KIND_IMAGE_REGISTRY="$mirror" \
    ALIEN_KIND_CONFIG="$config" ALIEN_KIND_NAME="$resource" ALIEN_KIND_OUT=workload.json \
    cargo test -p alien-infra --features all-platforms,test-utils --lib "${crate_path}::kubernetes::tests::${test_name}" -- --ignored >"$work/cargo.out" 2>&1) \
    || { grep -A6 panicked "$work/cargo.out" >&2; echo "could not render $example/$resource" >&2; exit 1; }
  enforce "$ns"
  kubectl -n "$ns" apply -f "$dir/workload.json"
  echo "applied $example/$resource ($kind) to $ns"
done

# Wait for every example pod to be Running, and require the mirror, by digest.
deadline=$((SECONDS + 300))
while :; do
  bad="$(kubectl get pods -A -o json | python3 -c '
import json, sys
bad = []
for pod in json.load(sys.stdin)["items"]:
    ns = pod["metadata"]["namespace"]
    if not ns.startswith("ex-"):
        continue
    if pod["status"].get("phase") != "Running":
        bad.append(ns + "/" + pod["metadata"]["name"] + ": " + pod["status"].get("phase", "?"))
    statuses = pod["status"].get("containerStatuses", [])
    if not statuses:
        bad.append(ns + "/" + pod["metadata"]["name"] + ": no container status yet")
    for status in statuses:
        if "running" not in status.get("state", {}):
            bad.append(ns + "/" + pod["metadata"]["name"] + " container " + status["name"] + " is not running: " + json.dumps(status.get("state")))
        # A crash-looping container is briefly "running" between restarts.
        if status.get("restartCount", 0) > 0 or not status.get("ready"):
            bad.append(ns + "/" + pod["metadata"]["name"] + " container " + status["name"] + " restarted or is not ready: restarts=" + str(status.get("restartCount", 0)) + " ready=" + str(status.get("ready")))
        if "@sha256:" not in status.get("imageID", "") or not status["imageID"].startswith("localhost:5001/"):
            bad.append(ns + "/" + pod["metadata"]["name"] + " imageID " + status.get("imageID", ""))
print("\n".join(bad))
')"
  pods="$(kubectl get pods -A --no-headers | grep -c '^ex-' || true)"
  [ -z "$bad" ] && [ "$pods" -ge "${#configs[@]}" ] && break
  [ "$SECONDS" -lt "$deadline" ] || { echo "example pods not Running from the mirror:" >&2; echo "$bad" >&2; kubectl get pods -A >&2; kubectl describe pods -A 2>&1 | grep -E "^Name:|Warning|Events" -A3 | tail -80 >&2; exit 1; }
  sleep 5
done
kubectl get pods -A | grep '^ex-' || { echo "no example pods were created" >&2; exit 1; }
echo "every example container and daemon runs in a restricted namespace"
printf 'images that run as root under their opt-out: %s\n' "${kube_images[*]:-none}"
echo "restricted pods and mirror: ok"
