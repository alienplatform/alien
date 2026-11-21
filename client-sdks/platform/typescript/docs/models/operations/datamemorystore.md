# DataMemorystore

GCP Memorystore for Valkey.

## Example Usage

```typescript
import { DataMemorystore } from "@alienplatform/platform-api/models/operations";

let value: DataMemorystore = {
  config: {},
  instanceName: "<value>",
  metrics: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "memorystore",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `config`                                                                                                                             | [operations.Config2](../../models/operations/config2.md)                                                                             | :heavy_check_mark:                                                                                                                   | Configuration the controller reads back from the cloud, to compare with the<br/>desired configuration.                               |
| `instanceName`                                                                                                                       | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |
| `metrics`                                                                                                                            | [operations.GetResourceDeploymentDetailMetrics2](../../models/operations/getresourcedeploymentdetailmetrics2.md)                     | :heavy_check_mark:                                                                                                                   | One sample of the numbers that matter for a Redis server. A field is `None`<br/>when the cloud did not return that metric in the window. |
| `state`                                                                                                                              | *string*                                                                                                                             | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `status`                                                                                                                             | [operations.GetResourceDeploymentDetailDataStatus38](../../models/operations/getresourcedeploymentdetaildatastatus38.md)             | :heavy_check_mark:                                                                                                                   | Same fields as every other resource heartbeat status.                                                                                |
| `backend`                                                                                                                            | *"memorystore"*                                                                                                                      | :heavy_check_mark:                                                                                                                   | N/A                                                                                                                                  |