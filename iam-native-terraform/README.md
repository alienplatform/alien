# Native Terraform IAM setup proof

Real AWS E2E target, eu-west-1. Terraform 1.9.8, hashicorp/aws 6.68.0,
hashicorp/random 3.9.1. Synthetic S3 object grants; no bucket/object writes.

The original aa42e2f38 CLI generated the oversized module from the published
`iam-render-oversized-alien.json` fixture (400 object ARNs). Its full module is
in `parent/`. The saved feature generator f60521756 produced `small/` from the ten-object
control. Both modules are unchanged except `providers_override.tf`, which
adds proof tags through AWS provider default_tags. No resources were removed
or targeted away. Local backend only; shared infrastructure state was not used.

With E2E target AWS credentials and a trusted management role supplied through
the environment, the exact commands were:

```bash
export TF_VAR_name=q5-t58-parent TF_VAR_resource_prefix=q5-t58-parent
export TF_VAR_token=q5-synthetic-unused TF_VAR_aws_region=eu-west-1
export TF_VAR_network_mode=use-default
# TF_VAR_managing_role_arn was supplied privately, not printed.
terraform init -input=false -no-color
terraform plan -input=false -no-color -out=q5-plan
terraform apply -input=false -no-color q5-plan
terraform destroy -input=false -no-color -auto-approve
```

Repeat in `small/` with both name/prefix variables set to `q5-t58-small`.
Both saved plans were checked as create-only, restricted to owned prefixed
IAM objects plus local random/terraform_data state. The default network was
read, not modified.

Parent apply exits 1: AWS PutRolePolicy409 LimitExceeded, maximum inline policy
size 10240. The generated document is 21317 characters. Provider inspection
confirmed two new roles and one managed policy, and no inline policy on the
execution role. This is a native Terraform error; Terraform does not invoke
Alien's runtime runner or client error mapping.

Feature 4f621b3d4 `alien render --format terraform --target aws` rejects the
same oversized input with exit 1 and zero artifact files, without cloud
credentials. It reports 21317/10240 before Terraform can run.

Small apply exits 0 with eight resources in Terraform state (including local
state). Provider GetRolePolicy confirms the expected ten grants / 647-character
inline policy. This proves native setup, including local registration data;
no runtime import or application deployment is claimed.

Both normal destroys exit 0. Final provider list-roles/list-policies confirms
zero owned prefixed objects and preservation of all baseline RoleIds/PolicyIds.
One unrelated test role appeared concurrently; it was excluded from ownership
and left untouched. No overrides or compute resources were created.

`proof-idfree.json` records assertions derived from raw provider receipts.
Logs and screenshots show the actual output with account IDs redacted; the
small screenshot selects creation/completion lines from the complete log.
