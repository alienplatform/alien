# Point

## Example Usage

```typescript
import { Point } from "@alienplatform/platform-api/models/operations";

let value: Point = {
  timestamp: "<value>",
  sourceStart: "<value>",
  sourceEnd: "<value>",
  requests: 2369.26,
  completed: 7957.2,
  aborted: 6106.69,
  probeExpired: 2042.41,
  captureStopped: 5109.56,
  unsupported: 4733.18,
  serverErrors: 2753.87,
  responseWireBytes: 982.78,
  rps: 5785.86,
  p95: {
    lower: 245.59,
    upper: 4425.35,
  },
  p99: {
    lower: 717.53,
    upper: null,
  },
  ttfbP95: {
    lower: 6631.9,
    upper: 884.18,
  },
  cpuPercent: 3467.7,
  memoryBytes: 2111.12,
  activeRequests: 3746.2,
  cpuPeakPercent: 84.16,
  memoryPeakBytes: 8758.02,
  activePeakRequests: 7356.52,
  statusCounts: {},
  availableReplicaSeconds: 1981.96,
  unavailableReplicaSeconds: 3474.25,
  captureQuality: [],
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `timestamp`                                              | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `sourceStart`                                            | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `sourceEnd`                                              | *string*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `requests`                                               | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `completed`                                              | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `aborted`                                                | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `probeExpired`                                           | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `captureStopped`                                         | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `unsupported`                                            | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `serverErrors`                                           | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `responseWireBytes`                                      | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `rps`                                                    | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `p95`                                                    | [operations.P95](../../models/operations/p95.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `p99`                                                    | [operations.P99](../../models/operations/p99.md)         | :heavy_check_mark:                                       | N/A                                                      |
| `ttfbP95`                                                | [operations.TtfbP95](../../models/operations/ttfbp95.md) | :heavy_check_mark:                                       | N/A                                                      |
| `cpuPercent`                                             | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `memoryBytes`                                            | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `activeRequests`                                         | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `cpuPeakPercent`                                         | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `memoryPeakBytes`                                        | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `activePeakRequests`                                     | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `statusCounts`                                           | Record<string, *number*>                                 | :heavy_check_mark:                                       | N/A                                                      |
| `availableReplicaSeconds`                                | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `unavailableReplicaSeconds`                              | *number*                                                 | :heavy_check_mark:                                       | N/A                                                      |
| `captureQuality`                                         | *number*[]                                               | :heavy_check_mark:                                       | N/A                                                      |