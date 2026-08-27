#!/usr/bin/env bash
#
# Removes everything the egress-deny guard creates, and fails if any of it survives — except a
# stack it deliberately keeps when a reclaim leaves resources behind, so the next run can find them.
#
# Runs before the guard as well as after it: a stack an earlier run left in a failed state would
# otherwise wedge every run that follows.
#
# Destructive, and assumes no guard run is in flight. The workflow serializes runs through a
# concurrency group; a hand-run invocation has nothing holding that lock.

set -uo pipefail

STACK_NAME=${1:?usage: egress-deny-guard-teardown.sh <stack-name>}

cd "$(dirname "$0")/.." || exit 1

run_ignored_test() {
  cargo nextest run -p alien-aws-clients --lib --no-tests=fail --run-ignored=all \
    -E "test(=aws::lambda_microvms::live_deny::$1)"
}

# An absent stack is the success case; anything else — expired credentials, AccessDenied, a
# throttle — must not be read as one. Matching the message is how `cleanup-aws-e2e-resources.sh`
# draws the same distinction.
stack_state() {
  local err
  if err=$(aws cloudformation describe-stacks --stack-name "$STACK_NAME" 2>&1 >/dev/null); then
    echo "present"
    return 0
  fi
  case "$err" in
    *"does not exist"*) echo "absent" ;;
    *) echo "::error::describe-stacks on $STACK_NAME was inconclusive: $err" >&2; return 1 ;;
  esac
}

state=$(stack_state) || exit 1
if [ "$state" = "absent" ]; then
  echo "no $STACK_NAME stack to remove"
  exit 0
fi

# A stack that never reached its change set carries no Outputs, which is expected here — that
# state is exactly what this run exists to clean up. A call that fails outright is not: reading it
# as "no outputs" would skip the reclaim and the image check while still exiting 0.
# stderr goes to its own file rather than into the value, so one stray CLI warning cannot make a
# well-formed stack look like a malformed one.
outputs_err=$(mktemp)
trap 'rm -f "$outputs_err"' EXIT
if ! resources=$(aws cloudformation describe-stacks --stack-name "$STACK_NAME" \
  --query "Stacks[0].Outputs[?OutputKey=='DeploymentResources'].OutputValue" \
  --output text 2>"$outputs_err"); then
  echo "::error::reading $STACK_NAME outputs was inconclusive: $(cat "$outputs_err")"
  exit 1
fi

image_arn=""
if [ -n "$resources" ] && [ "$resources" != "None" ]; then
  if ! image_arn=$(printf '%s' "$resources" |
    jq -re '.[] | select(.type == "sandbox") | .importData.imageArn' 2>/dev/null | head -1); then
    echo "::error::$STACK_NAME reported outputs that carry no sandbox image ARN"
    exit 1
  fi
fi

# Outputs are absent in the stack states this script exists to clear, so the image is resolved
# from the stack's own resources instead. Only a stack that genuinely declares no image may leave
# this empty: gating the reclaim and the is-it-gone check on a silently empty value would report a
# clean teardown over an image nobody looked at.
if [ -z "$image_arn" ]; then
  if ! image_arn=$(aws cloudformation list-stack-resources --stack-name "$STACK_NAME" \
    --query "StackResourceSummaries[?ResourceType=='AWS::Lambda::MicrovmImage'].PhysicalResourceId" \
    --output text 2>"$outputs_err" | head -1); then
    echo "::error::listing $STACK_NAME resources was inconclusive: $(cat "$outputs_err")"
    exit 1
  fi
  [ "$image_arn" = "None" ] && image_arn=""
fi

failed=0

# Sessions and image versions outlive the stack: the delete below reaches no running MicroVM, and
# a delete on the image is accepted while removing nothing for as long as a version survives.
# So versions go first, and the stack stays standing if that fails: it is the only way back to the
# image, whether through its outputs or its resources, and deleting it would leave MicroVMs
# billing with nothing left pointing at them.
if [ -n "$image_arn" ]; then
  echo "::add-mask::$image_arn"
  export PROBE_IMAGE_NAME="$image_arn"
  if ! run_ignored_test reclaim_the_probe_image; then
    echo "::error title=egressDeny guard::the reclaim did not clear the probe image, so \
$STACK_NAME stays up — it is what the next run needs to reach whatever is left. The failure \
above says whether resources survived or the reclaim could not run."
    exit 1
  fi
fi

aws cloudformation delete-stack --stack-name "$STACK_NAME" || failed=1
aws cloudformation wait stack-delete-complete --stack-name "$STACK_NAME" || failed=1

# Runs whatever the delete reported: a stack can reach DELETE_COMPLETE over an image the delete
# was accepted for and never removed, and this is the only thing that would notice.
if [ -n "$image_arn" ]; then
  run_ignored_test the_probe_image_is_gone || failed=1
fi

state=$(stack_state) || exit 1
if [ "$state" = "present" ]; then
  echo "::error title=egressDeny guard::$STACK_NAME survived its own teardown"
  exit 1
fi

exit "$failed"
