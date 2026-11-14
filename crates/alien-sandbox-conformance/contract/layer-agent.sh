#!/bin/sh
# Layers a locally built Alien sandbox agent onto the contract image, the way a released image
# carries it, so a Kind cluster can start the contract image under the agent.
#
#   layer-agent.sh <agent-binary> <contract-image> <output-tag> [exec-uid]
#
# The optional exec uid (default 1000) is the identity commands run as. Give it a non-root value
# such as 1001 to produce the image of a non-root declaration, which also ends as that USER.
# The agent must run on the contract image's libc (build it on the same distribution, or use the
# musl release binary). The image keeps no root-only agent mode: the pod chooses it.
set -eu
agent="$1"
base="$2"
tag="$3"
uid="${4:-1000}"
user_line=""
if [ "$uid" != 1000 ]; then user_line="USER $uid"; fi
context="$(mktemp -d)"
trap 'rm -rf "$context"' EXIT
cp "$agent" "$context/alien-sandbox-agent"
cat > "$context/Dockerfile" <<DOCKERFILE
FROM $base
COPY --chown=0:0 --chmod=0755 alien-sandbox-agent /usr/local/bin/alien-sandbox-agent
RUN test ! -L /etc/passwd && test ! -L /etc/group && test ! -L /sandbox \\
 && (awk -F: '\$3 == $uid { found=1 } END { exit !found }' /etc/passwd || printf 'sandbox:x:$uid:$uid::/sandbox:/sbin/nologin\n' >> /etc/passwd) \\
 && (awk -F: '\$3 == $uid { found=1 } END { exit !found }' /etc/group || printf 'sandbox:x:$uid:\n' >> /etc/group) \\
 && mkdir -p /sandbox && chown $uid:$uid /sandbox && chmod 0700 /sandbox \\
 && mkdir -p /opt/alien \\
 && printf '%s\n' '{"user":"","command":["python3","/opt/contract-conformance/boot.py"],"env":{"PATH":"/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"},"workingDirectory":"/"}' > /opt/alien/image-command.json \\
 && chmod 0600 /opt/alien/image-command.json
ENV ALIEN_SANDBOX_ROOT=/sandbox \\
    ALIEN_SANDBOX_PORT=8971 \\
    ALIEN_SANDBOX_AUTHORIZATION=transport \\
    ALIEN_SANDBOX_EXEC_UID=$uid \\
    ALIEN_SANDBOX_EXEC_GID=$uid \\
    ALIEN_SANDBOX_ISOLATION=platform
EXPOSE 8971/tcp
$user_line
ENTRYPOINT ["/usr/local/bin/alien-sandbox-agent"]
DOCKERFILE
docker build -q -t "$tag" "$context"
