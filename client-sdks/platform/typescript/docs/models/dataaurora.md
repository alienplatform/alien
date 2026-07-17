# DataAurora

AWS Aurora Serverless v2 backend.

## Example Usage

```typescript
import { DataAurora } from "@alienplatform/platform-api/models";

let value: DataAurora = {
  clusterIdentifier: "<value>",
  neverPauses: false,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "deleted",
    partial: true,
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
| `status`                                                                                                                                                    | [models.ResourceHeartbeatStatus33](../models/resourceheartbeatstatus33.md)                                                                                  | :heavy_check_mark:                                                                                                                                          | N/A                                                                                                                                                         |
| `backend`                                                                                                                                                   | *"aurora"*                                                                                                                                                  | :heavy_check_mark:                                                                                                                                          | N/A                                                                                                                                                         |