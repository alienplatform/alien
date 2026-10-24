
## Retain AWS setup data during uninstall

Setup-owned S3 buckets, DynamoDB tables and ECR repositories are protected against replacement and destroy. This includes empty resources. Their KMS keys must survive with them. Apply this package to the existing installation before teardown; older state may still allow destructive deletion. Do not remove protected resource blocks or bypass `prevent_destroy`: that protection disappears when a block is removed.

Use Python 3 and the same Terraform backend, workspace and working directory used to install this setup. Stop deployment workloads first. Serialize the following procedure with all other Terraform runs. For a child module, add `--module-address 'module.<name>'` (including any module instance index).

```bash
# Preview exactly which addresses and cloud IDs will be retained.
python3 ./retain-data.py --receipt-dir /secure/path/retained-setup
# Save private recovery state and detach only the generated retention inventory.
python3 ./retain-data.py --receipt-dir /secure/path/retained-setup --detach
terraform plan -destroy -out=uninstall.tfplan
terraform apply uninstall.tfplan
```

Review the destroy plan before applying: it must remove this setup's application/management roles and bucket policies, while the retained resources stay absent from the delete list. Detachment alone does not revoke access. Retained bucket encryption, object ownership, public-access-block and versioning settings stay in place; lifecycle-expiration configuration remains managed and is removed during destroy. Existing external grants are outside this setup's ownership and require a separate access review.

The receipt contains exact addresses, cloud IDs, state lineage and full before/after state. Keep it private and backed up: Terraform state can contain secrets. A retry creates a new receipt without overwriting earlier IDs. The helper fails on state-read errors and keeps Terraform locking enabled. Do not run ordinary `terraform apply` after detachment: it would try to recreate the resources. To resume management, restore their declarations and use `terraform import ADDRESS ID` for each retained instance, then review an in-place plan. Do not restore a full old state over newer state.

To remove just one data resource from the configuration, first inventory its state and use a reviewed Terraform `removed` block with `lifecycle { destroy = false }`, including its encryption dependencies. The bundled helper is for whole-setup teardown. Replacement requires retaining the old resource and importing or creating the new resource under a separate identity; never destroy old data to unblock a replacement plan.

Retained resources continue to incur cloud charges. To explicitly erase data, first finish access revocation, then use the cloud's deletion operations on the exact IDs in the receipt, including stored versions/images, only after confirming that no retained data depends on a key. This workflow covers the AWS setup resources listed above; PostgreSQL, other data types, runtime-owned resources and other clouds require their own retention procedure.
