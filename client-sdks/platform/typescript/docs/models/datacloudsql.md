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
        severity: "warning",
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

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `databaseVersion`                                                          | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `instanceName`                                                             | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `state`                                                                    | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus34](../models/resourceheartbeatstatus34.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"cloudSql"*                                                               | :heavy_check_mark:                                                         | N/A                                                                        |