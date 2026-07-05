# DataCloudSQL

## Example Usage

```typescript
import { DataCloudSQL } from "@alienplatform/platform-api/models/operations";

let value: DataCloudSQL = {
  instanceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "creating",
    partial: true,
    stale: true,
  },
  backend: "cloudSql",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `databaseVersion`                                                  | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `instanceName`                                                     | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `state`                                                            | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus32](../../models/operations/datastatus32.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"cloudSql"*                                                       | :heavy_check_mark:                                                 | N/A                                                                |