# AWS missing quota-read permission: artifact-render controls

The actual CLI runtime preflight reports Unknown warnings and continues when an installed identity cannot read applied quotas. These controls use a temporary tagged read-only role, with allowed EC2 discovery as a sentinel. The native quota API returns AccessDeniedException in the missing-permission phase; the allowed phase adds only servicequotas:GetServiceQuota.

CloudFormation and Terraform both exit 0 and generate artifacts (1 and 10 files respectively). Missing permissions produce warnings for VPC, internet gateway and EIP quotas. With quota reads allowed, both complete without quota warnings. Nothing is applied, and no network or compute objects are created by these commands.

Provenance: retained feature74 CLI, whose network quota reader and render warning path are unchanged by subsequent GCP/Lambda fixes. This is actual render/preflight behavior, not installed-update proof. Installed-update completion remains a separate gate; a real capacity-update attempt reached a downstream empty-cluster rollout stall, tracked separately.

Cleanup: exact-owned q5-compat94-reader inline policy and role deleted; GetRole returns NoSuchEntity. The IAM snapshot overlapped the separately tracked compute fixture: its two exact-owned role IDs explain the additions, and no unrelated IDs were added or removed. Final compute cleanup confirms all owned roles absent. Unrelated concurrent IAM drift prevents a full final-IAM-baseline claim; those resources were preserved.
