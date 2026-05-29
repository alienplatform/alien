# RemoteStackManagementHeartbeatDataAwsIamRole

## Example Usage

```typescript
import { RemoteStackManagementHeartbeatDataAwsIamRole } from "@alienplatform/manager-api/models";

let value: RemoteStackManagementHeartbeatDataAwsIamRole = {
  events: [],
  managementPermissionsApplied: true,
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
  backend: "awsIamRole",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `events`                                                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                           | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `managementPermissionsApplied`                                                                   | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `roleArn`                                                                                        | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `roleName`                                                                                       | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `status`                                                                                         | [models.RemoteStackManagementHeartbeatStatus](../models/remotestackmanagementheartbeatstatus.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `backend`                                                                                        | *"awsIamRole"*                                                                                   | :heavy_check_mark:                                                                               | N/A                                                                                              |