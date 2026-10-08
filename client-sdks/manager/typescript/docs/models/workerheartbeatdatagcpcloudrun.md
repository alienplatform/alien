# WorkerHeartbeatDataGcpCloudRun

## Example Usage

```typescript
import { WorkerHeartbeatDataGcpCloudRun } from "@alienplatform/manager-api/models";

let value: WorkerHeartbeatDataGcpCloudRun = {
  service: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  trafficCount: 570634,
  urls: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  backend: "gcpCloudRun",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `containerImage`                                                       | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `cpuLimit`                                                             | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `generation`                                                           | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `latestCreatedRevision`                                                | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `latestReadyRevision`                                                  | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `maxInstanceCount`                                                     | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `memoryLimit`                                                          | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `minInstanceCount`                                                     | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `observedGeneration`                                                   | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `region`                                                               | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `service`                                                              | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `status`                                                               | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `trafficCount`                                                         | *number*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `uri`                                                                  | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `urls`                                                                 | *string*[]                                                             | :heavy_check_mark:                                                     | N/A                                                                    |
| `backend`                                                              | *"gcpCloudRun"*                                                        | :heavy_check_mark:                                                     | N/A                                                                    |