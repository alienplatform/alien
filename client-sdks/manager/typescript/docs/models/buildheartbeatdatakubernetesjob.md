# BuildHeartbeatDataKubernetesJob

## Example Usage

```typescript
import { BuildHeartbeatDataKubernetesJob } from "@alienplatform/manager-api/models";

let value: BuildHeartbeatDataKubernetesJob = {
  conditionCount: 199682,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  jobName: "<value>",
  namespace: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "kubernetesJob",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `active`                                                                                      | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `completionTime`                                                                              | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `conditionCount`                                                                              | *number*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `events`                                                                                      | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `failed`                                                                                      | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `imageDigest`                                                                                 | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `jobName`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `namespace`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `startTime`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [models.BuildHeartbeatStatus](../models/buildheartbeatstatus.md)                              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `succeeded`                                                                                   | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *"kubernetesJob"*                                                                             | :heavy_check_mark:                                                                            | N/A                                                                                           |