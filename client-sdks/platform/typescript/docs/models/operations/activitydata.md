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
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `recent`                                                                             | [operations.ActivityRecent](../../models/operations/activityrecent.md)[]             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `investigations`                                                                     | [operations.Investigation](../../models/operations/investigation.md)[]               | :heavy_check_mark:                                                                   | Recent agent sessions about Remote Operator deployments or their access requests     |
| `debugSessions`                                                                      | [operations.DebugSession](../../models/operations/debugsession.md)[]                 | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `lastVerifiedOperation`                                                              | [operations.LastVerifiedOperation](../../models/operations/lastverifiedoperation.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |
