# PostgresHeartbeatDataLocal

Local embedded Postgres backend.

## Example Usage

```typescript
import { PostgresHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: PostgresHeartbeatDataLocal = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  version: "<value>",
  backend: "local",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `name`                                                                 | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `port`                                                                 | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `processRunning`                                                       | *boolean*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |
| `status`                                                               | [models.PostgresHeartbeatStatus](../models/postgresheartbeatstatus.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `version`                                                              | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `backend`                                                              | *"local"*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |