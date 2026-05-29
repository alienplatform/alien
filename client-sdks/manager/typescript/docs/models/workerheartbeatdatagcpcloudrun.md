# WorkerHeartbeatDataGcpCloudRun

## Example Usage

```typescript
import { WorkerHeartbeatDataGcpCloudRun } from "@alienplatform/manager-api/models";

let value: WorkerHeartbeatDataGcpCloudRun = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  service: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  trafficCount: 874910,
  urls: [
    "<value 1>",
    "<value 2>",
  ],
  backend: "gcpCloudRun",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `containerImage`                                                       | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `cpuLimit`                                                             | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `events`                                                               | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                 | :heavy_check_mark:                                                     | N/A                                                                    |
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