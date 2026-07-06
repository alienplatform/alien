# DataCloudSQL

## Example Usage

```typescript
import { DataCloudSQL } from "@alienplatform/platform-api/models";

let value: DataCloudSQL = {
  instanceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "scaling",
    partial: false,
    stale: true,
  },
  backend: "cloudSql",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `databaseVersion`                                          | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `instanceName`                                             | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `state`                                                    | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus32](../models/heartbeatstatus32.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"cloudSql"*                                               | :heavy_check_mark:                                         | N/A                                                        |