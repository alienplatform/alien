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
    investigations: [
      {
        id: "<id>",
        triggerType: "<value>",
        deploymentId: "<id>",
        status: "<value>",
        createdAt: new Date("2024-01-26T05:34:16.843Z"),
        updatedAt: new Date("2025-03-21T14:32:52.591Z"),
      },
    ],
    debugSessions: [
      {
        id: "<id>",
        deploymentId: "<id>",
        state: "New York",
        createdAt: new Date("2025-03-15T05:28:17.115Z"),
        expiresAt: new Date("2024-11-27T02:27:54.423Z"),
      },
    ],
    lastVerifiedOperation: {
      invocationId: "<id>",
      deploymentId: "<id>",
      commandId: "<id>",
      plugin: "<value>",
      pluginVersion: "<value>",
      operation: "<value>",
      tier: null,
      accessRequestId: "<id>",
      commandState: "SUCCEEDED",
      verificationState: "pending",
      createdAt: new Date("2024-12-20T02:15:50.612Z"),
      updatedAt: new Date("2024-03-28T12:17:18.067Z"),
      verification: {
        state: "verified",
        attempts: 533116,
        maxAttempts: 249676,
        deadline: new Date("2025-03-26T23:32:49.615Z"),
        reason: "<value>",
      },
      sensitiveOutput: {
        kind: "requireConfirmation",
      },
      resultAvailable: true,
      completedAt: new Date("2025-05-08T17:12:42.798Z"),
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