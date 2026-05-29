# DataAwsIamRole2

## Example Usage

```typescript
import { DataAwsIamRole2 } from "@alienplatform/platform-api/models";

let value: DataAwsIamRole2 = {
  events: [],
  managementPermissionsApplied: false,
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
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "awsIamRole",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `events`                                                                         | [models.SyncReconcileRequestEvent43](../models/syncreconcilerequestevent43.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `managementPermissionsApplied`                                                   | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `roleArn`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `roleName`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus43](../models/heartbeatstatus43.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"awsIamRole"*                                                                   | :heavy_check_mark:                                                               | N/A                                                                              |