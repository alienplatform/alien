# Activity

## Example Usage

```typescript
import { Activity } from "@alienplatform/platform-api/models/operations";

let value: Activity = {
  status: "empty",
  freshness: "unknown",
  sourceUpdatedAt: new Date("2025-03-02T01:20:36.654Z"),
  error: {
    code: "<value>",
    message: "<value>",
  },
  data: {
    recent: [],
    lastVerifiedOperation: {
      invocationId: "<id>",
      deploymentId: "<id>",
      commandId: "<id>",
      plugin: "<value>",
      pluginVersion: "<value>",
      operation: "<value>",
      tier: "read-only",
      commandState: "SUCCEEDED",
      verificationState: "pending",
      createdAt: new Date("2025-10-24T19:06:22.305Z"),
      updatedAt: new Date("2024-01-09T03:03:29.645Z"),
      verification: {
        state: "skipped",
        attempts: 455281,
        maxAttempts: 907051,
        deadline: new Date("2025-08-07T07:04:22.954Z"),
        reason: "<value>",
      },
      sensitiveOutput: {
        kind: "redact",
        fields: [
          "<value 1>",
        ],
      },
      resultAvailable: true,
      completedAt: null,
    },
  },
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `status`                                                                                      | [operations.ActivityStatus](../../models/operations/activitystatus.md)                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `freshness`                                                                                   | [operations.ActivityFreshness](../../models/operations/activityfreshness.md)                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `sourceUpdatedAt`                                                                             | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `error`                                                                                       | [operations.ActivityError](../../models/operations/activityerror.md)                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `data`                                                                                        | [operations.ActivityData](../../models/operations/activitydata.md)                            | :heavy_check_mark:                                                                            | N/A                                                                                           |