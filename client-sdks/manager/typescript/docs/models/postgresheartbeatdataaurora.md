# PostgresHeartbeatDataAurora

AWS Aurora Serverless v2 backend.

## Example Usage

```typescript
import { PostgresHeartbeatDataAurora } from "@alienplatform/manager-api/models";

let value: PostgresHeartbeatDataAurora = {
  clusterIdentifier: "<value>",
  neverPauses: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "aurora",
};
```

## Fields

| Field                                                                                                                                                       | Type                                                                                                                                                        | Required                                                                                                                                                    | Description                                                                                                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `clusterIdentifier`                                                                                                                                         | *string*                                                                                                                                                    | :heavy_check_mark:                                                                                                                                          | N/A                                                                                                                                                         |
| `endpoint`                                                                                                                                                  | *string*                                                                                                                                                    | :heavy_minus_sign:                                                                                                                                          | N/A                                                                                                                                                         |
| `engineVersion`                                                                                                                                             | *string*                                                                                                                                                    | :heavy_minus_sign:                                                                                                                                          | N/A                                                                                                                                                         |
| `neverPauses`                                                                                                                                               | *boolean*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                          | True when a `minCapacity: 0` instance has not reached 0 ACU over the observation<br/>window — it is silently paying always-on prices (auto-pause verification). |
| `serverlessCapacity`                                                                                                                                        | *number*                                                                                                                                                    | :heavy_minus_sign:                                                                                                                                          | Latest sampled `ServerlessDatabaseCapacity` (ACU).                                                                                                          |
| `status`                                                                                                                                                    | [models.PostgresHeartbeatStatus](../models/postgresheartbeatstatus.md)                                                                                      | :heavy_check_mark:                                                                                                                                          | N/A                                                                                                                                                         |
| `backend`                                                                                                                                                   | *"aurora"*                                                                                                                                                  | :heavy_check_mark:                                                                                                                                          | N/A                                                                                                                                                         |