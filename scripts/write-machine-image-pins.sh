#!/usr/bin/env bash
# Write the machine-image pins a release build compiles in
# (crates/alien-core/machine-image-*.txt) from the environment, and fail,
# naming every missing or malformed one, before anything is built. A build
# without them refuses every protected machine roll.
#
#   MACHINE_IMAGE_PUBLISHER_AWS_ACCOUNT    12-digit AWS account
#   MACHINE_IMAGE_PUBLISHER_GCP_PROJECT    GCP project id
#   MACHINE_IMAGE_PUBLISHER_AZURE_GALLERY  public name of the community gallery
#   MACHINE_IMAGE_MINIMUM_CREATED          oldest image creation time, RFC 3339 UTC
#                                          (e.g. 2026-11-01T00:00:00Z)
#
# The checks match what alien-core accepts, so a value written here is never
# silently read as "no pin".
set -euo pipefail

core="$(cd "$(dirname "$0")/../crates/alien-core" && pwd)"
errors=()

check() {
  local name="$1" pattern="$2" hint="$3"
  local value="${!name:-}"
  if [[ -z "$value" ]]; then
    errors+=("$name is not set ($hint)")
  elif ! [[ "$value" =~ $pattern ]]; then
    errors+=("$name='$value' is not $hint")
  fi
}

check MACHINE_IMAGE_PUBLISHER_AWS_ACCOUNT '^[0-9]{12}$' "a 12-digit AWS account"
check MACHINE_IMAGE_PUBLISHER_GCP_PROJECT '^[a-z0-9-]{1,30}$' "a GCP project id"
check MACHINE_IMAGE_PUBLISHER_AZURE_GALLERY '^[A-Za-z0-9-]{1,100}$' "a community gallery public name"
check MACHINE_IMAGE_MINIMUM_CREATED '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$' \
  "an RFC 3339 UTC time like 2026-11-01T00:00:00Z"
if [[ ${#errors[@]} -eq 0 ]] && ! date -u -d "$MACHINE_IMAGE_MINIMUM_CREATED" +%s >/dev/null 2>&1; then
  errors+=("MACHINE_IMAGE_MINIMUM_CREATED='$MACHINE_IMAGE_MINIMUM_CREATED' is not a real date")
fi

if [[ ${#errors[@]} -gt 0 ]]; then
  for error in "${errors[@]}"; do
    echo "::error::$error" >&2
  done
  echo "Set these repository variables before releasing: a verifier built without them refuses every protected machine roll." >&2
  exit 1
fi

printf '%s\n' "$MACHINE_IMAGE_PUBLISHER_AWS_ACCOUNT" > "$core/machine-image-publisher-aws.txt"
printf '%s\n' "$MACHINE_IMAGE_PUBLISHER_GCP_PROJECT" > "$core/machine-image-publisher-gcp.txt"
printf '%s\n' "$MACHINE_IMAGE_PUBLISHER_AZURE_GALLERY" > "$core/machine-image-publisher-azure.txt"
printf '%s\n' "$MACHINE_IMAGE_MINIMUM_CREATED" > "$core/machine-image-minimum-created.txt"
echo "machine-image pins written: aws=$MACHINE_IMAGE_PUBLISHER_AWS_ACCOUNT gcp=$MACHINE_IMAGE_PUBLISHER_GCP_PROJECT azure=$MACHINE_IMAGE_PUBLISHER_AZURE_GALLERY minimum=$MACHINE_IMAGE_MINIMUM_CREATED"
