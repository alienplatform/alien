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
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "awsIamRole",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `managementPermissionsApplied`                                             | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `roleArn`                                                                  | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `roleName`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus43](../models/resourceheartbeatstatus43.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"awsIamRole"*                                                             | :heavy_check_mark:                                                         | N/A                                                                        |