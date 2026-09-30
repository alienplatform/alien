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

# Flags every check passes to the script ahead of the image reference.
flags=()

check() {
  local mode="$1" expect="$2" annotation="$3" body="${4:-}"
  state="$(mktemp -d)"
  out="$(SMOKE_STUB_DIR="$state" SMOKE_STUB_MODE="$mode" "$script" ${flags[@]+"${flags[@]}"} alien-sandbox-agent:stub 2>&1)"
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
check image-error fail "linux/amd64: the digest reference could not be released before the next platform pull" "stub: the image cleanup failed"

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
removals=$(grep -c . "$state/image-removals" 2>/dev/null || true)
if [ "$removals" != 2 ]; then
  failed=$((failed + 1))
  echo "FAIL happy: digest reference removed ${removals} times, expected 2"
else
  passed=$((passed + 1))
  echo "ok   happy: digest reference removed between platform pulls"
fi

# The committed list, so a tool added to the contract is one this harness sends.
tools_list="$here/../docker/sandbox-default-tools.txt"
flags=(--tools "$tools_list")
# A timed-out probe's client is killed but its container is not, so each timeout path
# must remove the container by name.
for mode in tools-124 tools-hang tools-hang-rm-fails; do
  body=""
  case "$mode" in
    tools-hang) body="probing sleep 10" ;;
    tools-hang-rm-fails) body="::warning::linux/amd64: container smoke-tools-" ;;
  esac
  check "$mode" fail "linux/amd64: the tools probe did not finish within 120s" "$body"
  if grep -q "^-f smoke-tools-[0-9]*-linux-amd64$" "$state/container-removals" 2>/dev/null; then
    passed=$((passed + 1))
    echo "ok   ${mode}: the timed-out probe container was removed"
  else
    failed=$((failed + 1))
    echo "FAIL ${mode}: the timed-out probe container was never removed"
  fi
done
# A 137 well inside the budget is a container killed from outside, not our timeout.
check tools-137 fail "linux/amd64: the tools probe did not complete"
check tools-error fail "linux/amd64: stub: the tools probe failed"
check tools-silent fail "linux/amd64: the tools probe did not complete"
check tools-missing fail "linux/amd64: tool probe: rg --version failed"
check tools-hidden-failure fail "linux/amd64: tool probe: echo boom >&2; false # hidden failed: boom"
check tools-crash fail "linux/amd64: the tools probe stopped while running node --version"

check happy pass ""
for platform in linux/amd64 linux/arm64; do
  probes=$(grep -c "^${platform}$" "$state/platforms")
  if [ "$probes" -ne 7 ]; then
    failed=$((failed + 1))
    echo "FAIL tools happy: ${platform} probed ${probes} times, expected 7"
  else
    passed=$((passed + 1))
    echo "ok   tools happy: ${platform} probed 7 times"
  fi
done
missing=""
listed=0
while IFS= read -r line || [ -n "$line" ]; do
  line="${line%$'\r'}"
  line="${line#"${line%%[![:space:]]*}"}"
  case "$line" in ''|'#'*) continue ;; esac
  listed=$((listed + 1))
  grep -qF -- "$line" "$state/tools-probes" || missing+=" '${line}'"
done < "$tools_list"
if [ -n "$missing" ]; then
  failed=$((failed + 1))
  echo "FAIL tools happy: the probe never sent${missing}"
else
  passed=$((passed + 1))
  echo "ok   tools happy: the probe sent every command in the list"
fi
# One argument per tool, so no two commands share a shell.
if [ "$(sort -u "$state/tools-arguments")" != "$listed" ]; then
  failed=$((failed + 1))
  echo "FAIL tools happy: the probe got $(sort -u "$state/tools-arguments" | tr '\n' ' ')arguments, expected ${listed}"
else
  passed=$((passed + 1))
  echo "ok   tools happy: the probe got one argument per tool"
fi

empty_list="$(mktemp)"
printf '# only a comment\n  # indented\n\n' > "$empty_list"
flags=(--tools "$empty_list")
check tools-list-empty fail "the tools list ${empty_list} names no tools"
flags=(--tools "$empty_list.missing")
check tools-list-unreadable fail "the tools list ${empty_list}.missing is not readable"
rm -f "$empty_list"

flags=(--platforms linux/arm64)
check happy pass ""
amd64=$(grep -c "^linux/amd64$" "$state/platforms")
arm64=$(grep -c "^linux/arm64$" "$state/platforms")
removals=$(grep -c . "$state/image-removals" 2>/dev/null || true)
if [ "$amd64" -ne 0 ] || [ "$arm64" -ne 6 ] || [ "$removals" != 1 ]; then
  failed=$((failed + 1))
  echo "FAIL platforms: amd64 probed ${amd64}, arm64 ${arm64}, removals ${removals}; expected 0, 6, 1"
else
  passed=$((passed + 1))
  echo "ok   platforms: only linux/arm64 probed"
fi
flags=()

usage="usage: scripts/smoke-sandbox-agent.sh [--platforms <p1,p2>] [--tools <list-file>] <image-reference>"
for arguments in "" "--bogus alien-sandbox-agent:stub" "--tools" "one two" \
  "--platforms ,linux/arm64 alien-sandbox-agent:stub"; do
  # Word splitting is the point: each string is an argument list.
  # shellcheck disable=SC2086
  out="$("$script" $arguments 2>&1)"
  status=$?
  if [ "$status" -ne 2 ] || ! printf '%s\n' "$out" | grep -qF "$usage"; then
    report "usage '${arguments}'" "expected a usage error, exited ${status}"
  else
    passed=$((passed + 1))
    echo "ok   usage '${arguments}'"
  fi
done

echo
echo "${passed} passed, ${failed} failed"
[ "$failed" -eq 0 ]
