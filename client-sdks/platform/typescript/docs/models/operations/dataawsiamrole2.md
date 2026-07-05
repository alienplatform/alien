# DataAwsIamRole2

## Example Usage

```typescript
import { DataAwsIamRole2 } from "@alienplatform/platform-api/models/operations";

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

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `managementPermissionsApplied`                                     | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `roleArn`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `roleName`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus47](../../models/operations/datastatus47.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"awsIamRole"*                                                     | :heavy_check_mark:                                                 | N/A                                                                |