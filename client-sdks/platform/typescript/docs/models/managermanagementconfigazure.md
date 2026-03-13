# ManagerManagementConfigAzure

Azure management configuration extracted from stack settings

## Example Usage

```typescript
import { ManagerManagementConfigAzure } from "@aliendotdev/platform-api/models";

let value: ManagerManagementConfigAzure = {
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
| `platform`                                                          | [models.ManagerPlatformAzure](../models/managerplatformazure.md)    | :heavy_check_mark:                                                  | N/A                                                                 |