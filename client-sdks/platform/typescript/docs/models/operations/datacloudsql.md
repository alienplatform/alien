# DataCloudSQL

GCP Cloud SQL backend.

## Example Usage

```typescript
import { DataCloudSQL } from "@alienplatform/platform-api/models/operations";

let value: DataCloudSQL = {
  instanceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
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
| `status`                                                           | [operations.DataStatus34](../../models/operations/datastatus34.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"cloudSql"*                                                       | :heavy_check_mark:                                                 | N/A                                                                |