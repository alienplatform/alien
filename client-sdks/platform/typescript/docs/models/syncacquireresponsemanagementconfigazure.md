# SyncAcquireResponseManagementConfigAzure

Azure management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncAcquireResponseManagementConfigAzure } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseManagementConfigAzure = {
  managementPrincipalId: "<id>",
  managingTenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `managementPrincipalId`                                                                              | *string*                                                                                             | :heavy_check_mark:                                                                                   | The principal ID of the service principal in the management account                                  |
| `managingTenantId`                                                                                   | *string*                                                                                             | :heavy_check_mark:                                                                                   | The managing Azure Tenant ID for cross-tenant access                                                 |
| `platform`                                                                                           | [models.SyncAcquireResponseConfigPlatformAzure](../models/syncacquireresponseconfigplatformazure.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |