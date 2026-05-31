# DataAzureManagedIdentity2

## Example Usage

```typescript
import { DataAzureManagedIdentity2 } from "@alienplatform/platform-api/models";

let value: DataAzureManagedIdentity2 = {
  roleAssignmentIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "azureManagedIdentity",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `ficName`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `roleAssignmentIds`                                        | *string*[]                                                 | :heavy_check_mark:                                         | N/A                                                        |
| `roleDefinitionId`                                         | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus45](../models/heartbeatstatus45.md) | :heavy_check_mark:                                         | N/A                                                        |
| `tenantId`                                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `uamiClientId`                                             | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `uamiPrincipalId`                                          | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `uamiResourceId`                                           | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `backend`                                                  | *"azureManagedIdentity"*                                   | :heavy_check_mark:                                         | N/A                                                        |