# DataGcpCloudRun

## Example Usage

```typescript
import { DataGcpCloudRun } from "@alienplatform/platform-api/models";

let value: DataGcpCloudRun = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-03-08T10:07:05.119Z"),
      severity: "warning",
    },
  ],
  service: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  trafficCount: 335156,
  urls: [
    "<value 1>",
  ],
  backend: "gcpCloudRun",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `containerImage`                                                               | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `cpuLimit`                                                                     | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent6](../models/syncreconcilerequestevent6.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `generation`                                                                   | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `latestCreatedRevision`                                                        | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `latestReadyRevision`                                                          | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `maxInstanceCount`                                                             | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `memoryLimit`                                                                  | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `minInstanceCount`                                                             | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `observedGeneration`                                                           | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `region`                                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `service`                                                                      | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `status`                                                                       | [models.HeartbeatStatus6](../models/heartbeatstatus6.md)                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `trafficCount`                                                                 | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `uri`                                                                          | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `urls`                                                                         | *string*[]                                                                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"gcpCloudRun"*                                                                | :heavy_check_mark:                                                             | N/A                                                                            |