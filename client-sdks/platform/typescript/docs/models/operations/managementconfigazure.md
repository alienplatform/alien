# ManagementConfigAzure

Azure management configuration extracted from stack settings

## Example Usage

```typescript
import { ManagementConfigAzure } from "@alienplatform/platform-api/models/operations";

let value: ManagementConfigAzure = {
  managementPrincipalId: "<id>",
  managingTenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `managementPrincipalId`                                                                        | *string*                                                                                       | :heavy_check_mark:                                                                             | The principal ID of the service principal in the management account                            |
| `managingTenantId`                                                                             | *string*                                                                                       | :heavy_check_mark:                                                                             | The managing Azure Tenant ID for cross-tenant access                                           |
| `platform`                                                                                     | [operations.CreateManagerPlatformAzure](../../models/operations/createmanagerplatformazure.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |