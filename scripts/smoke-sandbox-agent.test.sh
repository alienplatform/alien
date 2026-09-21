#!/usr/bin/env bash
#
# Run scripts/smoke-sandbox-agent.sh against a stub docker, once per mode, and
# assert what each one does: a failure exits nonzero AND names itself in a
# ::error:: annotation, a success exits zero with no annotation at all. A mode
# that dies without an annotation reads as a pass to a release run, so the two
# assertions are one check.
#
# Usage: scripts/smoke-sandbox-agent.test.sh
#
# Needs no Docker daemon and no image.
set -uo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="$here/smoke-sandbox-agent.sh"
stubs="$here/smoke-sandbox-agent.test.d"

SMOKE_REAL_TIMEOUT="$(command -v timeout || command -v gtimeout)"
if [ -z "$SMOKE_REAL_TIMEOUT" ]; then
  echo "no coreutils timeout on PATH; install coreutils" >&2
  exit 1
fi
export SMOKE_REAL_TIMEOUT
export SMOKE_STUB_TIMEOUT=3
PATH="$stubs:$PATH"

passed=0
failed=0
state=""
out=""

report() {
  failed=$((failed + 1))
  echo "FAIL ${1}: ${2}"
  printf '%s\n' "$out" | sed 's/^/     | /'
}

check() {
  local mode="$1" expect="$2" annotation="$3" body="${4:-}"
  state="$(mktemp -d)"
  out="$(SMOKE_STUB_DIR="$state" SMOKE_STUB_MODE="$mode" "$script" alien-sandbox-agent:stub 2>&1)"
  local status=$?

  if [ "$expect" = pass ]; then
    if [ "$status" -ne 0 ]; then
      report "$mode" "expected success, exited ${status}"
      return
    fi
    if printf '%s\n' "$out" | grep -q '^::error::'; then
      report "$mode" "succeeded but still annotated an error"
      return
    fi
  else
    if [ "$status" -eq 0 ]; then
      report "$mode" "expected failure, exited 0"
      return
    fi
    if ! printf '%s\n' "$out" | grep -qF "::error::${annotation}"; then
      report "$mode" "no annotation reading '::error::${annotation}'"
      return
    fi
  fi
  if [ -n "$body" ] && ! printf '%s\n' "$out" | grep -qF "$body"; then
    report "$mode" "output does not carry '${body}'"
    return
  fi
  passed=$((passed + 1))
  echo "ok   ${mode}"
}

check pull-124 fail "linux/amd64: the agent image did not pull within 300s"
check pull-137 fail "linux/amd64: the agent image did not pull within 300s"
check pull-error fail "linux/amd64: the agent image could not be pulled" "stub: the pull probe failed"

check header-124 fail "linux/amd64: the header read did not finish within 60s"
check header-137 fail "linux/amd64: the header read did not finish within 60s"
check header-error fail "linux/amd64: the header read failed with exit 7"
check header-mismatch fail "linux/amd64: the agent is e_machine 'b700', not '3e00'"

check identity-error fail "linux/amd64: stub: the identity probe failed"
check identity-silent fail "linux/amd64: the identity probe did not complete"

check unlink-124 fail "linux/amd64: the unlink probe did not finish within 60s"
check unlink-137 fail "linux/amd64: the unlink probe did not finish within 60s"
check unlink-error fail "linux/amd64: /sandbox is not writable by the exec uid" "stub: the unlink probe failed"
check unlink-silent fail "linux/amd64: /sandbox is not writable by the exec uid" "the unlink probe said nothing"

check reject-accepted fail "linux/amd64: the agent started with an invalid authorization mode"
check reject-124 fail "linux/amd64: the agent did not exit within 60s on an invalid authorization mode"
check reject-137 fail "linux/amd64: the agent did not exit within 60s on an invalid authorization mode"
check reject-unnamed-code fail "linux/amd64: the agent's rejection names no AGENT_CONFIG_INVALID"
check reject-unnamed-variable fail "linux/amd64: the agent's rejection names no ALIEN_SANDBOX_AUTHORIZATION"

check detached-124 fail "linux/amd64: the detached run did not return a container within 60s"
check detached-137 fail "linux/amd64: the detached run did not return a container within 60s"
check detached-error fail "linux/amd64: the agent container did not start with exit 7"
check detached-warning pass ""

check listener-never fail "linux/amd64: the agent never reported a listener within 60s"
check inspect-unreadable fail "linux/amd64: the container state could not be read, so the listener proves nothing"
check listener-then-exit fail "linux/amd64: the agent reported a listener and then exited"

check happy pass ""
# Every mode above asserts on linux/amd64, so this is the only thing that would
# notice a probe which stopped running on the other architecture. A floor rather
# than the exact count lets such a probe go missing with every mode still green.
for platform in linux/amd64 linux/arm64; do
  probes=$(grep -c "^${platform}$" "$state/platforms")
  if [ "$probes" -ne 6 ]; then
    failed=$((failed + 1))
    echo "FAIL happy: ${platform} probed ${probes} times, expected 6"
  else
    passed=$((passed + 1))
    echo "ok   happy: ${platform} probed 6 times"
  fi
done
removals=$(wc -l < "$state/image-removals")
if [ "$removals" -ne 2 ]; then
  failed=$((failed + 1))
  echo "FAIL happy: digest reference removed ${removals} times, expected 2"
else
  passed=$((passed + 1))
  echo "ok   happy: digest reference removed between platform pulls"
fi

out="$("$script" 2>&1)"
status=$?
if [ "$status" -eq 0 ] || ! printf '%s\n' "$out" | grep -qF "usage: scripts/smoke-sandbox-agent.sh <image-reference>"; then
  report no-argument "expected a usage error, exited ${status}"
else
  passed=$((passed + 1))
  echo "ok   no-argument"
fi

echo
echo "${passed} passed, ${failed} failed"
[ "$failed" -eq 0 ]
