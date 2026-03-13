# SyncReconcileResponseManagementConfigAzure

Azure management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncReconcileResponseManagementConfigAzure } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseManagementConfigAzure = {
  managementPrincipalId: "<id>",
  managingTenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                               | Type                                                                | Required                                                            | Description                                                         |
| ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `managementPrincipalId`                                             | *string*                                                            | :heavy_check_mark:                                                  | The principal ID of the service principal in the management account |
| `managingTenantId`                                                  | *string*                                                            | :heavy_check_mark:                                                  | The managing Azure Tenant ID for cross-tenant access                |
| `platform`                                                          | [models.TargetPlatformAzure](../models/targetplatformazure.md)      | :heavy_check_mark:                                                  | N/A                                                                 |