# RemoteStackManagementHeartbeatDataAwsIamRole

## Example Usage

```typescript
import { RemoteStackManagementHeartbeatDataAwsIamRole } from "@alienplatform/manager-api/models";

let value: RemoteStackManagementHeartbeatDataAwsIamRole = {
  managementPermissionsApplied: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "awsIamRole",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `managementPermissionsApplied`                                                                   | *boolean*                                                                                        | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `roleArn`                                                                                        | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `roleName`                                                                                       | *string*                                                                                         | :heavy_minus_sign:                                                                               | N/A                                                                                              |
| `status`                                                                                         | [models.RemoteStackManagementHeartbeatStatus](../models/remotestackmanagementheartbeatstatus.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `backend`                                                                                        | *"awsIamRole"*                                                                                   | :heavy_check_mark:                                                                               | N/A                                                                                              |