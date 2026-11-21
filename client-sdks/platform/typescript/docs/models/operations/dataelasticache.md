# DataElasticache

AWS ElastiCache for Valkey.

## Example Usage

```typescript
import { DataElasticache } from "@alienplatform/platform-api/models/operations";

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
| `config`                                                                                                                             | [operations.Config1](../../models/operations/config1.md)                                                                             | :heavy_check_mark:                                                                                                                   | Configuration the controller reads back from the cloud, to compare with the<br/>desired configuration.                               |
| `metrics`                                                                                                                            | [operations.GetResourceDeploymentDetailMetrics1](../../models/operations/getresourcedeploymentdetailmetrics1.md)                     | :heavy_check_mark:                                                                                                                   | One sample of the numbers that matter for a Redis server. A field is `None`<br/>when the cloud did not return that metric in the window. |
| `replicationGroupId`                                                                                                                 | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
| `state`                                                                                                                              | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `status`                                                                                                                             | [operations.GetResourceDeploymentDetailDataStatus37](../../models/operations/getresourcedeploymentdetaildatastatus37.md)             | :heavy_check_mark:                                                                                                                   | Same fields as every other resource heartbeat status.                                                                                |
| `backend`                                                                                                                            | *"elasticache"*                                                                                                                      | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |