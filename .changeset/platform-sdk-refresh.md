---
"@alienplatform/platform-api": major
---

Regenerate the platform TypeScript SDK from the current platform API.

Breaking changes:

- `billing.listAuditLog` and its `BillingAuditLogRow`, `ListBillingAuditLog*`
  models and React Query hooks are removed. The platform API no longer serves
  `GET /v1/billing/audit-log`.
- `PlanId.ProAnnual` is removed and `PlanId.Internal` is added.
- `WorkspaceBillingEntitlements.billableQuantities` and the dynamic container
  logs response's `cursorRestarted` are now required response fields.
- `projects.getActiveRelease` returns a validated `Release` instead of `any`.
- `ExternalBindingUnion` gains a `local-queue-http` variant.

New operations: `billing.sync`, `getDeploymentMetricsHistory`,
`requestDeploymentCheck`, `resumeDeploymentInstallation`,
`releases.listChannelImpact`,
`deploymentGroups.getDeploymentGroupManagementExternalId`,
`sync.managementExternalId`, `acceptAwsKeySetup`, `listAwsKeySetupCleanup`,
`claimAwsXksStoreRetirement` and `completeAwsXksStoreRetirement`.
