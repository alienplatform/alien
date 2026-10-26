# Cloud Storage controller qualification

Use one disposable, public-capable project. This test creates one bucket and
briefly makes a synthetic object public. It does not change organization policy.
Prerequisites: Cargo, gcloud, jq, Python 3, setup credentials authorized to create
test identities, and a fresh `test-...` prefix. Never reuse an existing bucket or
identity. Record the names before creating anything.

Set `ALIEN_STORAGE_LIVE_FIXTURE` to a private JSON file with these fields:

```json
{
  "projectId": "YOUR_DISPOSABLE_PROJECT",
  "projectNumber": "YOUR_PROJECT_NUMBER",
  "region": "us-central1",
  "prefix": "test-UNIQUE_LOWERCASE_SUFFIX",
  "serviceAccount": "YOUR_FRESH_ACCOUNT@YOUR_DISPOSABLE_PROJECT.iam.gserviceaccount.com",
  "observerMember": "user:YOUR_SETUP_USER_EMAIL",
  "setupTokenFile": "/absolute/private/setup.token",
  "runtimeTokenFile": "/absolute/private/runtime.token",
  "evidenceDir": "/absolute/private/evidence"
}
```

Keep the directory mode0700 and token files mode0600. Verify the project number
and account email before writes. Export the exact role and binding plan:

```sh
cargo test -p alien-infra --all-features --test gcp_storage_live export_grants -- --ignored
```

Create the fresh service account and install only `grants.json` from the evidence
directory: its `customRoles` are project custom roles, and its `bindings` specify
the role, member and target. Do not substitute Owner or Storage Admin. If setup
needs impersonation, grant Token Creator only on this fresh account to the setup
principal. Mint a short-lived cloud-platform token for the account into
`runtimeTokenFile`; never put the setup token there. Wait for the newly created
identity and grants to become visible before starting the test. Keep propagation
waits bounded; unrelated permission errors are failures.

```sh
cargo test -p alien-infra --all-features --test gcp_storage_live public_private -- --ignored --nocapture
```

Run cleanup even if that command fails. The following Bash commands use the same
setup token file and exact recorded fixture names. Run them only for resources
you created for this fixture. They deliberately attempt identity cleanup even
when bucket cleanup fails; a failed command must remain in the test receipt.
For interrupted setup, execute the applicable commands for the objects actually
recorded as created, and independently verify any NotFound result.

```bash
test_project=$(jq -er .projectId "$ALIEN_STORAGE_LIVE_FIXTURE")
test_bucket="$(jq -er .prefix "$ALIEN_STORAGE_LIVE_FIXTURE")-assets"
test_account=$(jq -er .serviceAccount "$ALIEN_STORAGE_LIVE_FIXTURE")
test_setup_token=$(jq -er .setupTokenFile "$ALIEN_STORAGE_LIVE_FIXTURE")
test_runtime_token=$(jq -er .runtimeTokenFile "$ALIEN_STORAGE_LIVE_FIXTURE")
test_evidence=$(jq -er .evidenceDir "$ALIEN_STORAGE_LIVE_FIXTURE")
gc() { gcloud --project="$test_project" --access-token-file="$test_setup_token" "$@"; }

cleanup_failed=0
gc storage buckets update "gs://$test_bucket" --clear-soft-delete || cleanup_failed=1
sleep 35
# --recursive includes noncurrent object versions and the bucket itself.
gc storage rm --recursive "gs://$test_bucket" || cleanup_failed=1
while IFS= read -r test_role; do
  gc projects remove-iam-policy-binding "$test_project" \
    --member="serviceAccount:$test_account" --role="$test_role" \
    --condition=None --quiet || cleanup_failed=1
  gc iam roles delete "${test_role##*/}" --quiet || cleanup_failed=1
done < <(jq -er '.customRoles[].name' "$test_evidence/grants.json")
gc iam service-accounts delete "$test_account" --quiet || cleanup_failed=1
rm -f -- "$test_runtime_token"
printf 'cleanup command failure flag: %s\n' "$cleanup_failed"
```

Check the project IAM policy for absence of the exact account member, describe
the account and bucket to confirm NotFound, and describe each custom role to
confirm `deleted: true`. Normal bucket NotFound is insufficient: query the JSON
API `GET /storage/v1/b?project=PROJECT&prefix=BUCKET&softDeleted=true` with setup
credentials and save its response without the Authorization header. If it still
lists the exact bucket, record its generation and `hardDeleteTime`; permanent
erasure is not proven. Do not repeat restore/delete loops or change shared
policies. Keep project deletion with the owner of the disposable project.

Provider references: [recursive removal](https://docs.cloud.google.com/sdk/gcloud/reference/storage/rm),
[disabling soft deletion](https://docs.cloud.google.com/storage/docs/disable-soft-delete),
and [IAM propagation](https://docs.cloud.google.com/iam/docs/access-change-propagation).
