# GetDeploymentMetricsHistoryResponse

Metrics history

## Example Usage

```typescript
import { GetDeploymentMetricsHistoryResponse } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentMetricsHistoryResponse = {
  capability: "plaintext-http1",
  bucketSeconds: 5221.23,
  boundarySemantics: "source-window-end",
  points: [
    {
      timestamp: "<value>",
      sourceStart: "<value>",
      sourceEnd: "<value>",
      requests: 746.98,
      completed: 3381.74,
      aborted: 6819.94,
      probeExpired: 2554.29,
      captureStopped: 4363.21,
      unsupported: null,
      serverErrors: null,
      responseWireBytes: 7262.34,
      rps: 9401.87,
      p95: {
        lower: 0.01,
        upper: 0.025,
      },
      p99: {
        lower: 0.01,
        upper: 0.025,
      },
      ttfbP95: {
        lower: 0.01,
        upper: 0.025,
      },
      cpuPercent: 13.74,
      memoryBytes: 3321.37,
      activeRequests: 9824.6,
      cpuPeakPercent: 8784.27,
      memoryPeakBytes: 6644.46,
      activePeakRequests: 9485.13,
      statusCounts: {
        "key": 7015.16,
      },
      availableReplicaSeconds: 8624.28,
      unavailableReplicaSeconds: 6669.61,
      captureQuality: [
        3044.71,
      ],
    },
  ],
  services: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  releases: [
    "<value 1>",
  ],
  replicas: [
    "<value 1>",
    "<value 2>",
  ],
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `capability`                                                                 | [operations.Capability](../../models/operations/capability.md)               | :heavy_check_mark:                                                           | N/A                                                                          |
| `bucketSeconds`                                                              | *number*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `boundarySemantics`                                                          | [operations.BoundarySemantics](../../models/operations/boundarysemantics.md) | :heavy_check_mark:                                                           | N/A                                                                          |
| `points`                                                                     | [operations.Point](../../models/operations/point.md)[]                       | :heavy_check_mark:                                                           | N/A                                                                          |
| `services`                                                                   | *string*[]                                                                   | :heavy_check_mark:                                                           | N/A                                                                          |
| `releases`                                                                   | *string*[]                                                                   | :heavy_check_mark:                                                           | N/A                                                                          |
| `replicas`                                                                   | *string*[]                                                                   | :heavy_check_mark:                                                           | N/A                                                                          |