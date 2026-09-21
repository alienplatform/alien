#!/usr/bin/env bash
#
# Qualify a published alien-sandbox-agent image on both platforms it ships for:
# the architecture it actually contains, the identity and filesystem it grants
# the supervised command, its rejection of an invalid config, and a real run
# that reaches its listener and stays up.
#
# Usage: scripts/smoke-sandbox-agent.sh <image-reference>
set -euo pipefail

image="${1:?usage: scripts/smoke-sandbox-agent.sh <image-reference>}"

for platform in linux/amd64 linux/arm64; do
  # This image is wolfi-base plus git's 24 transitive packages plus the agent, and the
  # amd64 half arrives under emulation. Inside a probe's own budget, the pull expires.
  status=0
  pull=$(timeout -k 5 300 docker pull --platform "$platform" "$image" 2>&1) || status=$?
  case "$status" in
    0) ;;
    124|137)
      echo "::error::${platform}: the agent image did not pull within 300s"
      exit 1 ;;
    *)
      echo "$pull"
      echo "::error::${platform}: the agent image could not be pulled"
      exit 1 ;;
  esac

  # Once binfmt is registered an ELF of either architecture executes whatever the
  # container declares, so running the agent cannot tell the two apart. The ELF header
  # can: e_machine sits at offset 18, 0x3e for x86-64 and 0xb7 for aarch64.
  case "$platform" in
    linux/amd64) expected=3e00 ;;
    linux/arm64) expected=b700 ;;
    *) echo "::error::no expected e_machine for ${platform}"; exit 1 ;;
  esac
  status=0
  machine=$(timeout -k 5 60 docker run --rm --platform "$platform" --entrypoint /bin/sh "$image" \
    -c 'od -An -tx1 -j18 -N2 /usr/local/bin/alien-sandbox-agent' | tr -d ' \n') || status=$?
  case "$status" in
    0) ;;
    124|137)
      echo "::error::${platform}: the header read did not finish within 60s"
      exit 1 ;;
    *)
      echo "::error::${platform}: the header read failed with exit ${status}"
      exit 1 ;;
  esac
  if [ "$machine" != "$expected" ]; then
    echo "::error::${platform}: the agent is e_machine '${machine}', not '${expected}'"
    exit 1
  fi

  # The failing run below never reaches the port parse and overrides the authorization
  # mode, so both of those are read off the image here instead.
  # The probe's expansions belong to the container's shell, not this one.
  # shellcheck disable=SC2016
  if ! identity=$(timeout -k 5 60 docker run --rm --platform "$platform" --entrypoint /bin/sh "$image" -c '
        id | grep -q "uid=1000(sandbox) gid=1000(sandbox)" ||
          { echo "runs as $(id), not uid=1000(sandbox) gid=1000(sandbox)"; exit 1; }
        test "$ALIEN_SANDBOX_AUTHORIZATION" = transport ||
          { echo "ALIEN_SANDBOX_AUTHORIZATION is ${ALIEN_SANDBOX_AUTHORIZATION:-unset}, not transport"; exit 1; }
        test "$ALIEN_SANDBOX_PORT" = 8080 ||
          { echo "ALIEN_SANDBOX_PORT is ${ALIEN_SANDBOX_PORT:-unset}, not 8080"; exit 1; }
      ' 2>&1); then
    echo "::error::${platform}: ${identity:-the identity probe did not complete}"
    exit 1
  fi
  # The unlink rather than the file mode: only the directory permission stops the
  # supervised command removing its own supervisor. Removing a file it may remove first,
  # so a denial is what fails the rm and not an rm that is missing or broken.
  status=0
  unlink=$(timeout -k 5 60 docker run --rm --platform "$platform" --entrypoint /bin/sh "$image" \
    -c 'touch /sandbox/probe && rm /sandbox/probe &&
        ! rm -f /usr/local/bin/alien-sandbox-agent &&
        test -x /usr/local/bin/alien-sandbox-agent' 2>&1) || status=$?
  case "$status" in
    0) ;;
    124|137)
      echo "::error::${platform}: the unlink probe did not finish within 60s"
      exit 1 ;;
    *)
      echo "${unlink:-the unlink probe said nothing}"
      echo "::error::${platform}: /sandbox is not writable by the exec uid, or that uid can unlink its own supervisor"
      exit 1 ;;
  esac
  # test -x reads a mode bit, which a binary that cannot start still carries. Matching
  # identifiers only the agent emits is what proves the entrypoint is a working agent.
  status=0
  out=$(timeout -k 5 60 docker run --rm --platform "$platform" \
    -e ALIEN_SANDBOX_AUTHORIZATION=nonsense "$image" 2>&1) || status=$?
  # timeout SIGTERMs the docker client, which ignores it, so -k is what ends a wedged
  # run: 124 when TERM took, 137 when the follow-up SIGKILL did. Not reachable today,
  # since load_state fails before any bind; it guards a bind moved ahead of validation.
  case "$status" in
    0)
      echo "::error::${platform}: the agent started with an invalid authorization mode"
      exit 1 ;;
    124|137)
      echo "::error::${platform}: the agent did not exit within 60s on an invalid authorization mode"
      exit 1 ;;
  esac
  for token in AGENT_CONFIG_INVALID ALIEN_SANDBOX_AUTHORIZATION; do
    case "$out" in
      *"$token"*) ;;
      *)
        echo "$out"
        echo "::error::${platform}: the agent's rejection names no ${token}"
        exit 1 ;;
    esac
  done

  # Every run above is built to fail. Started as it ships, the listening line means
  # attribution_works() returned true under USER 1000:1000 and the bind took. A dead
  # container's log reads the same, so the inspect is what makes the line mean it still.
  status=0
  container=$(timeout -k 5 60 docker run -d --platform "$platform" "$image") || status=$?
  case "$status" in
    0) ;;
    124|137)
      echo "::error::${platform}: the detached run did not return a container within 60s"
      exit 1 ;;
    *)
      echo "::error::${platform}: the agent container did not start with exit ${status}"
      exit 1 ;;
  esac
  listening=0
  if timeout -k 5 60 sh -c \
      "until docker logs ${container} 2>&1 | grep -q 'sandbox agent listening on'; do sleep 1; done"; then
    listening=1
  fi
  # The inspect is instantaneous, so an agent that logs the line and dies a second
  # later reads as serving. The dwell is what makes .State.Running mean it is still up.
  sleep 3
  inspected=0
  running=$(docker inspect -f '{{.State.Running}}' "$container" 2>&1) || inspected=$?
  logs=$(docker logs "$container" 2>&1 || true)
  docker rm -f "$container" >/dev/null || true
  if [ "$listening" -ne 1 ]; then
    echo "$logs"
    echo "::error::${platform}: the agent never reported a listener within 60s"
    exit 1
  fi
  if [ "$inspected" -ne 0 ]; then
    echo "$running"
    echo "::error::${platform}: the container state could not be read, so the listener proves nothing"
    exit 1
  fi
  if [ "$running" != true ]; then
    echo "$logs"
    echo "::error::${platform}: the agent reported a listener and then exited"
    exit 1
  fi

  # A digest reference can name only one locally selected platform in the classic
  # Docker image store. Remove that reference before pulling the other platform;
  # layers remain cached, while Docker no longer rejects the second pull with
  # "cannot overwrite digest".
  if ! cleanup=$(docker image rm "$image" 2>&1); then
    echo "$cleanup"
    echo "::error::${platform}: the digest reference could not be released before the next platform pull"
    exit 1
  fi
done
