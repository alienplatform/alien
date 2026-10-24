
## Retain GCP setup data during uninstall

Setup-owned Cloud Storage buckets, Firestore databases, Artifact Registry repositories and Cloud SQL PostgreSQL databases are protected against replacement and destroy, including empty resources. Cloud SQL retention includes the SQL database, login, password secret and version. Encryption keys, their key rings and Cloud Storage's service-agent encryption grant survive with encrypted buckets. Application, public-read and management IAM grants remain managed for removal.

Apply the current package to the existing installation before uninstalling. Stop deployment workloads first. Use Python 3 on Linux or macOS, or Windows through WSL with receipts on its Linux filesystem, and the original Terraform backend, workspace and working directory. Serialize this procedure with other Terraform runs. For a child module add `--module-address 'module.<name>'`, including any module instance index.

```bash
python3 ./retain-data.py --receipt-dir /secure/path/retained-setup
python3 ./retain-data.py --receipt-dir /secure/path/retained-setup --detach
terraform plan -destroy -out=uninstall.tfplan
terraform apply uninstall.tfplan
```

Review the destroy plan before applying: retained data must be absent from its delete list, while owned IAM grants, service accounts and Cloud SQL PSC consumer endpoints are removed. Detachment alone does not revoke access. Existing external grants are outside this setup's ownership and require a separate access review. Cloud SQL remains private; reconnecting requires an administrator to authorize a new private endpoint and access to the retained credential. The consumer VPC, subnet and endpoint are not storage dependencies of the Cloud SQL instance and are not retained.

Cloud Storage lifecycle expiry is part of the bucket itself. Before detaching, remove delete rules from the declaration and apply that change. The helper refuses to detach a bucket whose Terraform state still contains delete rules. Refresh the installed state before this procedure so provider-side changes are included.

Receipts include exact addresses, IDs, state lineage and full before/after Terraform state, which may contain secrets. Keep them private and backed up. Retries preserve earlier receipts. After detachment do not run ordinary `terraform apply`, which would try to recreate resources. To resume management, restore declarations, import each retained instance at its recorded address and ID, and review an in-place plan. Never overwrite newer state with an old full state file.

For configuration removal, use reviewed Terraform `removed` blocks with `lifecycle { destroy = false }` for the data and its required encryption dependencies. The helper is for whole-setup teardown. Do not delete declarations or bypass replacement guards to force a replacement: keep old data under a separate identity.

Retained databases, storage, repositories, secrets and keys continue to incur charges. Explicit erasure is a separate administrator operation after access revocation: use the exact recorded cloud IDs, including object/secret versions and images, and verify no retained data needs a key before destroying key versions. Cloud SQL's Terraform deletion protection can be disabled in a deliberately reimported configuration before explicit deletion. This workflow covers the GCP setup resources listed above; runtime-owned resources require their own retention procedure.
