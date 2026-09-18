# DataAzureContainerApps2

## Example Usage

```typescript
import { DataAzureContainerApps2 } from "@alienplatform/platform-api/models/operations";

let value: DataAzureContainerApps2 = {
  environmentVariableCount: 246098,
  managedEnvironmentId: "<id>",
  resourceGroupName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  backend: "azureContainerApps",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `environmentVariableCount`                                         | *number*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `managedEnvironmentId`                                             | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `managedIdentityId`                                                | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `resourceGroupName`                                                | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `resourcePrefix`                                                   | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus58](../../models/operations/datastatus58.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"azureContainerApps"*                                             | :heavy_check_mark:                                                 | N/A                                                                |