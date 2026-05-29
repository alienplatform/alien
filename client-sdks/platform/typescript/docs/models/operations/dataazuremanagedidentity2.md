# DataAzureManagedIdentity2

## Example Usage

```typescript
import { DataAzureManagedIdentity2 } from "@alienplatform/platform-api/models/operations";

let value: DataAzureManagedIdentity2 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-06-15T03:37:32.834Z"),
      severity: "info",
    },
  ],
  roleAssignmentIds: [],
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "updating",
    partial: false,
    stale: true,
  },
  backend: "azureManagedIdentity",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent45](../../models/operations/getrawresourceheartbeatevent45.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `ficName`                                                                                                | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `roleAssignmentIds`                                                                                      | *string*[]                                                                                               | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `roleDefinitionId`                                                                                       | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus45](../../models/operations/datastatus45.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `tenantId`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `uamiClientId`                                                                                           | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `uamiPrincipalId`                                                                                        | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `uamiResourceId`                                                                                         | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"azureManagedIdentity"*                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |