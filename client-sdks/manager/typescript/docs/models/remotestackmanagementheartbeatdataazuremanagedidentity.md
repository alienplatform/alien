# RemoteStackManagementHeartbeatDataAzureManagedIdentity

## Example Usage

```typescript
import { RemoteStackManagementHeartbeatDataAzureManagedIdentity } from "@alienplatform/manager-api/models";

let value: RemoteStackManagementHeartbeatDataAzureManagedIdentity = {
  roleAssignmentIds: [
    "<value 1>",
    "<value 2>",
  ],
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "azureManagedIdentity",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `ficName`                                                                                        | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `roleAssignmentIds`                                                                              | *string*[]                                                                                       | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `roleDefinitionId`                                                                               | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `status`                                                                                         | [models.RemoteStackManagementHeartbeatStatus](../models/remotestackmanagementheartbeatstatus.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `tenantId`                                                                                       | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `uamiClientId`                                                                                   | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `uamiPrincipalId`                                                                                | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `uamiResourceId`                                                                                 | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `backend`                                                                                        | *"azureManagedIdentity"*                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |