# RemoteStackManagementHeartbeatDataAzureManagedIdentity

## Example Usage

```typescript
import { RemoteStackManagementHeartbeatDataAzureManagedIdentity } from "@alienplatform/manager-api/models";

let value: RemoteStackManagementHeartbeatDataAzureManagedIdentity = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  roleAssignmentIds: [
    "<value 1>",
    "<value 2>",
  ],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "azureManagedIdentity",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `events`                                                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                           | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `ficName`                                                                                        | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `roleAssignmentIds`                                                                              | *string*[]                                                                                       | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `roleDefinitionId`                                                                               | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `status`                                                                                         | [models.RemoteStackManagementHeartbeatStatus](../models/remotestackmanagementheartbeatstatus.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `tenantId`                                                                                       | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `uamiClientId`                                                                                   | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `uamiPrincipalId`                                                                                | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `uamiResourceId`                                                                                 | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `backend`                                                                                        | *"azureManagedIdentity"*                                                                         | :heavy_check_mark:                                                                               | N/A                                                                                              |