# RemoteStackManagementHeartbeatDataGcpServiceAccount

## Example Usage

```typescript
import { RemoteStackManagementHeartbeatDataGcpServiceAccount } from "@alienplatform/manager-api/models";

let value: RemoteStackManagementHeartbeatDataGcpServiceAccount = {
  events: [],
  impersonationGranted: true,
  roleBound: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "gcpServiceAccount",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `events`                                                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                           | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `impersonationGranted`                                                                           | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `roleBound`                                                                                      | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `serviceAccountEmail`                                                                            | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `serviceAccountUniqueId`                                                                         | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `status`                                                                                         | [models.RemoteStackManagementHeartbeatStatus](../models/remotestackmanagementheartbeatstatus.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `backend`                                                                                        | *"gcpServiceAccount"*                                                                            | :heavy_check_mark:                                                                               | N/A                                                                                              |