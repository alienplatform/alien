# DataGcpServiceAccount2

## Example Usage

```typescript
import { DataGcpServiceAccount2 } from "@alienplatform/platform-api/models";

let value: DataGcpServiceAccount2 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-09-28T13:47:01.596Z"),
      severity: "error",
    },
  ],
  impersonationGranted: true,
  roleBound: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "gcpServiceAccount",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `events`                                                                         | [models.SyncReconcileRequestEvent44](../models/syncreconcilerequestevent44.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `impersonationGranted`                                                           | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `roleBound`                                                                      | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `serviceAccountEmail`                                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `serviceAccountUniqueId`                                                         | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus44](../models/heartbeatstatus44.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"gcpServiceAccount"*                                                            | :heavy_check_mark:                                                               | N/A                                                                              |