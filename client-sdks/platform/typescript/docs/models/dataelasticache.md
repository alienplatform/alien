# DataElasticache

AWS ElastiCache for Valkey.

## Example Usage

```typescript
import { DataElasticache } from "@alienplatform/platform-api/models";

let value: DataElasticache = {
  config: {},
  metrics: {},
  replicationGroupId: "<id>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "running",
    partial: true,
    stale: false,
  },
  backend: "elasticache",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `config`                                                                                                                             | [models.SyncReconcileRequestConfig1](../models/syncreconcilerequestconfig1.md)                                                       | :heavy_check_mark:                                                                                                                   | Configuration the controller reads back from the cloud, to compare with the<br/>desired configuration.                               |
| `metrics`                                                                                                                            | [models.SyncReconcileRequestMetrics1](../models/syncreconcilerequestmetrics1.md)                                                     | :heavy_check_mark:                                                                                                                   | One sample of the numbers that matter for a Redis server. A field is `None`<br/>when the cloud did not return that metric in the window. |
| `replicationGroupId`                                                                                                                 | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
| `state`                                                                                                                              | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `status`                                                                                                                             | [models.ResourceHeartbeatStatus37](../models/resourceheartbeatstatus37.md)                                                           | :heavy_check_mark:                                                                                                                   | Same fields as every other resource heartbeat status.                                                                                |
| `backend`                                                                                                                            | *"elasticache"*                                                                                                                      | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |