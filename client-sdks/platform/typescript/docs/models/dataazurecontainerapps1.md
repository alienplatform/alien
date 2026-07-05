# DataAzureContainerApps1

## Example Usage

```typescript
import { DataAzureContainerApps1 } from "@alienplatform/platform-api/models";

let value: DataAzureContainerApps1 = {
  appName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "azureContainerApps",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `appName`                                                                | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `cpu`                                                                    | *number*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `environmentName`                                                        | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `ingressFqdn`                                                            | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `maxReplicas`                                                            | *number*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `memory`                                                                 | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `minReplicas`                                                            | *number*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `provisioningState`                                                      | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `revision`                                                               | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `runningStatus`                                                          | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `status`                                                                 | [models.ResourceHeartbeatStatus7](../models/resourceheartbeatstatus7.md) | :heavy_check_mark:                                                       | N/A                                                                      |
| `backend`                                                                | *"azureContainerApps"*                                                   | :heavy_check_mark:                                                       | N/A                                                                      |