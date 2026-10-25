
## Retain Azure setup data during uninstall

Setup-owned storage accounts, Blob containers, tables, container registries, Key Vaults and PostgreSQL Flexible Servers are protected against replacement and destroy, including empty resources. The recovery inventory includes encryption keys, the storage encryption identity and its key grant, database credentials and extensions. The resource group is retained because deleting it would cascade to retained children. Application and management identities and role assignments remain managed for removal.

Apply the current package to the existing installation before uninstalling. Stop deployment workloads first. Set each owned container's `publicRead` to `false` and apply; the helper refuses public containers. Set `azure_storage_lifecycle_enabled = false` in the installed Terraform variables and apply. Keep the disabled policy through detachment. Refresh state before detachment. The helper refuses applicable delete rules on detached accounts or containers, while allowing unrelated container rules and tiering-only actions; it cannot infer past expiry from an empty current policy. Review runtime-created containers and policies separately.

Use Python 3 on Linux or macOS, or Windows through WSL with receipts on its Linux filesystem, and the original Terraform backend, workspace and working directory. Serialize this procedure with other Terraform runs. For a child module add `--module-address 'module.<name>'`, including its instance index.

```bash
python3 ./retain-data.py --receipt-dir /secure/path/retained-setup
python3 ./retain-data.py --receipt-dir /secure/path/retained-setup --detach
terraform plan -destroy -out=uninstall.tfplan
terraform apply uninstall.tfplan
```

Review the plan before applying: retained data and its group must be absent from deletes; owned access roles, workload identities and private endpoints must be removed. Detachment alone does not revoke access. Existing external grants remain outside this setup's ownership. Revoke outstanding user-delegation keys and rotate both storage account keys if credentials or SAS were issued; removing RBAC alone does not invalidate every issued credential. Verify anonymous and prior workload access is denied before declaring teardown complete.

PostgreSQL stays private. Its Private Endpoint, DNS and consumer network are access paths and are removed, while the database, credential vault and server remain. An administrator can reconnect by authorizing a new private endpoint and credential access. Storage encryption identity and key permission stay because the retained account needs them to decrypt data.

Receipts contain exact addresses, IDs, state lineage and full before/after Terraform state, which may contain secrets. Keep them private and backed up. Retries preserve earlier receipts. Do not run ordinary `terraform apply` after whole-setup detachment: it would try to recreate resources. Restore declarations and import each retained instance at its recorded address and ID before resuming management; review an in-place plan. Never overwrite newer state with an old state file.

Retained resources continue to incur charges. Explicit erasure is a separate administrator operation against recorded cloud IDs after access revocation; preserve keys until all dependent data is erased. Key Vault purge protection still enforces its recovery window. This procedure covers the setup resources listed above; runtime-owned data requires its own retention procedure.
