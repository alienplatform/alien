# DataAwsIamRole2

## Example Usage

```typescript
import { DataAwsIamRole2 } from "@alienplatform/platform-api/models/operations";

let value: DataAwsIamRole2 = {
  events: [],
  managementPermissionsApplied: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "awsIamRole",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent43](../../models/operations/getrawresourceheartbeatevent43.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `managementPermissionsApplied`                                                                           | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `roleArn`                                                                                                | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `roleName`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus43](../../models/operations/datastatus43.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"awsIamRole"*                                                                                           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |