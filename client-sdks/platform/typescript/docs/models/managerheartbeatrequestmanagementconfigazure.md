# ManagerHeartbeatRequestManagementConfigAzure

Azure management configuration extracted from stack settings

## Example Usage

```typescript
import { ManagerHeartbeatRequestManagementConfigAzure } from "@alienplatform/platform-api/models";

let value: ManagerHeartbeatRequestManagementConfigAzure = {
  managementPrincipalId: "<id>",
  managingTenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `managementPrincipalId`                                                                          | *string*                                                                                         | :heavy_check_mark:                                                                               | The principal ID of the service principal in the management account                              |
| `managingTenantId`                                                                               | *string*                                                                                         | :heavy_check_mark:                                                                               | The managing Azure Tenant ID for cross-tenant access                                             |
| `platform`                                                                                       | [models.ManagerHeartbeatRequestPlatformAzure](../models/managerheartbeatrequestplatformazure.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |