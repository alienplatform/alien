#!/bin/sh
# Layers a locally built Alien sandbox agent onto the contract image, the way a released image
# carries it, so a Kind cluster can start the contract image under the agent.
#
#   layer-agent.sh <agent-binary> <contract-image> <output-tag>
#
# The agent must run on the contract image's libc (build it on the same distribution, or use the
# musl release binary). The image keeps no root-only agent mode: the pod chooses it.
set -eu
agent="$1"
base="$2"
tag="$3"
context="$(mktemp -d)"
trap 'rm -rf "$context"' EXIT
cp "$agent" "$context/alien-sandbox-agent"
cat > "$context/Dockerfile" <<DOCKERFILE
FROM $base
COPY --chown=0:0 --chmod=0755 alien-sandbox-agent /usr/local/bin/alien-sandbox-agent
RUN test ! -L /etc/passwd && test ! -L /etc/group && test ! -L /sandbox \\
 && (awk -F: '\$3 == 1000 { found=1 } END { exit !found }' /etc/passwd || printf 'sandbox:x:1000:1000::/sandbox:/sbin/nologin\n' >> /etc/passwd) \\
 && (awk -F: '\$3 == 1000 { found=1 } END { exit !found }' /etc/group || printf 'sandbox:x:1000:\n' >> /etc/group) \\
 && mkdir -p /sandbox && chown 1000:1000 /sandbox && chmod 0700 /sandbox \\
 && mkdir -p /opt/alien \\
 && printf '%s\n' '{"user":"","command":["python3","/opt/contract-conformance/boot.py"],"env":{"PATH":"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"},"workingDirectory":"/"}' > /opt/alien/image-command.json \\
 && chmod 0600 /opt/alien/image-command.json
ENV ALIEN_SANDBOX_ROOT=/sandbox \\
    ALIEN_SANDBOX_PORT=8971 \\
    ALIEN_SANDBOX_AUTHORIZATION=transport \\
    ALIEN_SANDBOX_EXEC_UID=1000 \\
    ALIEN_SANDBOX_EXEC_GID=1000 \\
    ALIEN_SANDBOX_ISOLATION=platform
EXPOSE 8971/tcp
ENTRYPOINT ["/usr/local/bin/alien-sandbox-agent"]
DOCKERFILE
docker build -q -t "$tag" "$context"
