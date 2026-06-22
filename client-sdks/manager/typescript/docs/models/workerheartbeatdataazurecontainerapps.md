# WorkerHeartbeatDataAzureContainerApps

## Example Usage

```typescript
import { WorkerHeartbeatDataAzureContainerApps } from "@alienplatform/manager-api/models";

let value: WorkerHeartbeatDataAzureContainerApps = {
  appName: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
  backend: "azureContainerApps",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `appName`                                                              | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `cpu`                                                                  | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `environmentName`                                                      | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `ingressFqdn`                                                          | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `maxReplicas`                                                          | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `memory`                                                               | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `minReplicas`                                                          | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `provisioningState`                                                    | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `revision`                                                             | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `runningStatus`                                                        | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `status`                                                               | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `backend`                                                              | *"azureContainerApps"*                                                 | :heavy_check_mark:                                                     | N/A                                                                    |