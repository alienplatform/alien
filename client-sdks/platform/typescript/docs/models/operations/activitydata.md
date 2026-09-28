# ActivityData

## Example Usage

```typescript
import { ActivityData } from "@alienplatform/platform-api/models/operations";

let value: ActivityData = {
  recent: [],
  investigations: [],
  debugSessions: [],
  lastVerifiedOperation: {
    invocationId: "<id>",
    deploymentId: "<id>",
    commandId: "<id>",
    plugin: "<value>",
    pluginVersion: "<value>",
    operation: "<value>",
    tier: null,
    commandState: "SUCCEEDED",
    verificationState: "verified",
    createdAt: new Date("2024-12-22T11:58:06.410Z"),
    updatedAt: new Date("2024-12-20T02:15:50.612Z"),
    verification: {
      state: "skipped",
      attempts: 371867,
      maxAttempts: 533116,
      deadline: new Date("2024-06-09T09:14:41.952Z"),
      reason: "<value>",
    },
    sensitiveOutput: {
      kind: "none",
    },
    resultAvailable: false,
    completedAt: new Date("2025-12-16T19:53:47.814Z"),
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `recent`                                                                             | [operations.ActivityRecent](../../models/operations/activityrecent.md)[]             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `investigations`                                                                     | [operations.Investigation](../../models/operations/investigation.md)[]               | :heavy_check_mark:                                                                   | Recent agent sessions about Remote Operator deployments or their access requests     |
| `debugSessions`                                                                      | [operations.DebugSession](../../models/operations/debugsession.md)[]                 | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `lastVerifiedOperation`                                                              | [operations.LastVerifiedOperation](../../models/operations/lastverifiedoperation.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |