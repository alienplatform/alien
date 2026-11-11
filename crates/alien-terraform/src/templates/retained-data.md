
## Retain AWS setup data during uninstall

Setup-owned S3 buckets, DynamoDB tables, ECR repositories, ElastiCache Redis and Aurora PostgreSQL are protected against replacement and destroy. This includes empty resources. Their KMS keys survive with them. For Redis that is the replication group, its AUTH-token secret, and the subnet group, parameter group and security group AWS will not delete while the group exists. For Aurora that is the cluster (with its managed password secret), the writer instance, and the same three supporting resources. When the setup created the network, its VPC and private subnets are kept too, because the retained network interfaces live there; an existing or default VPC is never touched. Application and management IAM grants, alarms, gateways, route tables and security-group rules are removed during teardown. Aurora keeps no automatic final snapshot: take one yourself before erasing it. Apply this package to the existing installation before teardown; older state may still allow destructive deletion. Removing data declarations from an installed configuration is not supported by this helper. `prevent_destroy` protection disappears when a block is removed; keep the installed declarations until whole-setup detachment. Never bypass protection to force a replacement.

Use Python 3 on Linux or macOS (on Windows, use WSL with receipts on its Linux filesystem) and the same Terraform backend, workspace and working directory used to install this setup. Stop deployment workloads first. Serialize the following procedure with all other Terraform runs. For a child module, add `--module-address 'module.<name>'` (including any module instance index).

```bash
# Preview exactly which addresses and cloud IDs will be retained.
python3 ./retain-data.py --receipt-dir /secure/path/retained-setup
# Save private recovery state and detach only the generated retention inventory.
python3 ./retain-data.py --receipt-dir /secure/path/retained-setup --detach
terraform plan -destroy -out=uninstall.tfplan
terraform apply uninstall.tfplan
```

Review the destroy plan before applying: it must remove this setup's application/management roles and bucket policies, while the retained resources stay absent from the delete list. Detachment alone does not revoke access. Retained bucket encryption, object ownership, public-access-block and versioning settings stay in place; lifecycle-expiration configuration remains managed and is removed during destroy. Existing external grants are outside this setup's ownership and require a separate access review.

The receipt contains exact addresses, cloud IDs, state lineage and full before/after state. Keep it private and backed up: Terraform state can contain secrets. A retry creates a new receipt without overwriting earlier IDs. The helper fails on state-read errors and keeps Terraform locking enabled. Do not run ordinary `terraform apply` after whole-setup detachment: it would try to recreate the resources. To resume management, restore their declarations and use `terraform import ADDRESS ID` for each retained instance, then review an in-place plan. Do not restore a full old state over newer state.



Retained resources continue to incur cloud charges, including storage and KMS. To explicitly erase data, first finish access revocation, then use the cloud's deletion operations on the exact IDs in the receipt, including stored versions/images, only after confirming that no retained data depends on a key. This workflow covers the AWS setup resources listed above. For ElastiCache and Aurora, delete the replication group, then the Aurora instance and cluster, then the supporting groups, secrets, subnets and VPC by their recorded IDs; take an Aurora snapshot first if you want one. Other database backends, other data types, runtime-owned resources and other clouds require their own retention procedure. DynamoDB tables with point-in-time recovery keep an AWS-managed backup for up to 35 days after the table is deleted.

When retaining DynamoDB tables, install AWS CLI and use the same target-account installer credentials as Terraform. The helper verifies the recorded table ARN, disables its observed TTL attribute and requires AWS to report `DISABLED` before removing any selected resource from state. If TTL is transitioning, or AWS is still processing an earlier TTL change (including the one at table creation; this takes up to an hour), wait and rerun the same command; no resources are detached. Required metadata/control permissions are `DescribeTable`, `DescribeTimeToLive` and `UpdateTimeToLive`.
