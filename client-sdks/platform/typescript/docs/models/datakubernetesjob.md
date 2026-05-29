# DataKubernetesJob

## Example Usage

```typescript
import { DataKubernetesJob } from "@alienplatform/platform-api/models";

let value: DataKubernetesJob = {
  conditionCount: 902553,
  events: [],
  jobName: "<value>",
  namespace: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "scaling",
    partial: false,
    stale: true,
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
| `events`                                                                                      | [models.SyncReconcileRequestEvent53](../models/syncreconcilerequestevent53.md)[]              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `failed`                                                                                      | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `imageDigest`                                                                                 | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `jobName`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `namespace`                                                                                   | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `startTime`                                                                                   | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [models.HeartbeatStatus53](../models/heartbeatstatus53.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `succeeded`                                                                                   | *number*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *"kubernetesJob"*                                                                             | :heavy_check_mark:                                                                            | N/A                                                                                           |