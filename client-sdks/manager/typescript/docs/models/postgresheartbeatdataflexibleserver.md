# PostgresHeartbeatDataFlexibleServer

Azure Flexible Server backend.

## Example Usage

```typescript
import { PostgresHeartbeatDataFlexibleServer } from "@alienplatform/manager-api/models";

let value: PostgresHeartbeatDataFlexibleServer = {
  serverName: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "flexibleServer",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `serverName`                                                           | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `state`                                                                | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `status`                                                               | [models.PostgresHeartbeatStatus](../models/postgresheartbeatstatus.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `version`                                                              | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `backend`                                                              | *"flexibleServer"*                                                     | :heavy_check_mark:                                                     | N/A                                                                    |