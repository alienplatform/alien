# DataAwsIamRole2

## Example Usage

```typescript
import { DataAwsIamRole2 } from "@alienplatform/platform-api/models";

let value: DataAwsIamRole2 = {
  managementPermissionsApplied: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: false,
  },
  backend: "awsIamRole",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `managementPermissionsApplied`                             | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `roleArn`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `roleName`                                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus47](../models/heartbeatstatus47.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"awsIamRole"*                                             | :heavy_check_mark:                                         | N/A                                                        |