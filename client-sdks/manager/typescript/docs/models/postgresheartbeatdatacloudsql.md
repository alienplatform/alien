# PostgresHeartbeatDataCloudSQL

GCP Cloud SQL backend.

## Example Usage

```typescript
import { PostgresHeartbeatDataCloudSQL } from "@alienplatform/manager-api/models";

let value: PostgresHeartbeatDataCloudSQL = {
  instanceName: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "cloudSql",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `databaseVersion`                                                      | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `instanceName`                                                         | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `state`                                                                | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `status`                                                               | [models.PostgresHeartbeatStatus](../models/postgresheartbeatstatus.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `backend`                                                              | *"cloudSql"*                                                           | :heavy_check_mark:                                                     | N/A                                                                    |