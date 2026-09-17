# ActivityData

## Example Usage

```typescript
import { ActivityData } from "@alienplatform/platform-api/models/operations";

let value: ActivityData = {
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
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `recent`                                                                             | [operations.ActivityRecent](../../models/operations/activityrecent.md)[]             | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `lastVerifiedOperation`                                                              | [operations.LastVerifiedOperation](../../models/operations/lastverifiedoperation.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |