# DataAzureManagedRedis

Azure Managed Redis.

## Example Usage

```typescript
import { DataAzureManagedRedis } from "@alienplatform/platform-api/models/operations";

let value: DataAzureManagedRedis = {
  clusterName: "<value>",
  config: {},
  metrics: {},
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "azureManagedRedis",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `clusterName`                                                                                                                        | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
| `config`                                                                                                                             | [operations.Config3](../../models/operations/config3.md)                                                                             | :heavy_check_mark:                                                                                                                   | Configuration the controller reads back from the cloud, to compare with the<br/>desired configuration.                               |
| `metrics`                                                                                                                            | [operations.GetResourceDeploymentDetailMetrics3](../../models/operations/getresourcedeploymentdetailmetrics3.md)                     | :heavy_check_mark:                                                                                                                   | One sample of the numbers that matter for a Redis server. A field is `None`<br/>when the cloud did not return that metric in the window. |
| `state`                                                                                                                              | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `status`                                                                                                                             | [operations.GetResourceDeploymentDetailDataStatus39](../../models/operations/getresourcedeploymentdetaildatastatus39.md)             | :heavy_check_mark:                                                                                                                   | Same fields as every other resource heartbeat status.                                                                                |
| `backend`                                                                                                                            | *"azureManagedRedis"*                                                                                                                | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |