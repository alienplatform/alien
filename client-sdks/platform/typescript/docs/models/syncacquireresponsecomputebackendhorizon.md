# SyncAcquireResponseComputeBackendHorizon

Compute backend for Container and Worker resources.

Determines how compute workloads are orchestrated on cloud platforms.
When None, the platform default is used for cloud platforms.

## Example Usage

```typescript
import { SyncAcquireResponseComputeBackendHorizon } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseComputeBackendHorizon = {
  clusters: {
    "key": {
      clusterId: "<id>",
      managementToken: "<value>",
    },
  },
  url: "https://lanky-jazz.com",
  type: "horizon",
};
```

## Fields

| Field                                                                                                                                                       | Type                                                                                                                                                        | Required                                                                                                                                                    | Description                                                                                                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `clusters`                                                                                                                                                  | Record<string, [models.SyncAcquireResponseClusters](../models/syncacquireresponseclusters.md)>                                                              | :heavy_check_mark:                                                                                                                                          | Cluster configurations (one per ComputeCluster resource)<br/>Key: ComputeCluster resource ID from stack<br/>Value: Cluster ID and management token for that cluster |
| `horizonHostImage`                                                                                                                                          | *models.SyncAcquireResponseHorizonHostImageUnion*                                                                                                           | :heavy_minus_sign:                                                                                                                                          | N/A                                                                                                                                                         |
| `url`                                                                                                                                                       | *string*                                                                                                                                                    | :heavy_check_mark:                                                                                                                                          | Horizon control-plane API base URL.                                                                                                                         |
| `type`                                                                                                                                                      | [models.SyncAcquireResponseComputeBackendType](../models/syncacquireresponsecomputebackendtype.md)                                                          | :heavy_check_mark:                                                                                                                                          | N/A                                                                                                                                                         |