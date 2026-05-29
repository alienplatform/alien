# DataAzureContainerApps1

## Example Usage

```typescript
import { DataAzureContainerApps1 } from "@alienplatform/platform-api/models/operations";

let value: DataAzureContainerApps1 = {
  appName: "<value>",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-05-02T10:27:21.624Z"),
      severity: "warning",
    },
  ],
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  backend: "azureContainerApps",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `appName`                                                                                              | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `cpu`                                                                                                  | *number*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `environmentName`                                                                                      | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `events`                                                                                               | [operations.GetRawResourceHeartbeatEvent7](../../models/operations/getrawresourceheartbeatevent7.md)[] | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `ingressFqdn`                                                                                          | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `maxReplicas`                                                                                          | *number*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `memory`                                                                                               | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `minReplicas`                                                                                          | *number*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `provisioningState`                                                                                    | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `revision`                                                                                             | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `runningStatus`                                                                                        | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `status`                                                                                               | [operations.DataStatus7](../../models/operations/datastatus7.md)                                       | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `backend`                                                                                              | *"azureContainerApps"*                                                                                 | :heavy_check_mark:                                                                                     | N/A                                                                                                    |