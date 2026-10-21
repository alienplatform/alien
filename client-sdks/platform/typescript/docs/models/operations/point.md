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
    lower: 0.01,
    upper: 0.025,
  },
  p99: null,
  ttfbP95: {
    lower: 0.01,
    upper: 0.025,
  },
  cpuPercent: 9939.21,
  memoryBytes: null,
  activeRequests: null,
  cpuPeakPercent: 6631.9,
  memoryPeakBytes: 884.18,
  activePeakRequests: 3467.7,
  statusCounts: {
    "key": 2111.12,
    "key1": 6140.12,
    "key2": 3746.2,
  },
  availableReplicaSeconds: 7050.06,
  unavailableReplicaSeconds: 84.16,
  captureQuality: [
    8758.02,
  ],
};
```

## Fields

| Field                                                                         | Type                                                                          | Required                                                                      | Description                                                                   |
| ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `timestamp`                                                                   | *string*                                                                      | :heavy_check_mark:                                                            | N/A                                                                           |
| `sourceStart`                                                                 | *string*                                                                      | :heavy_check_mark:                                                            | Null means no source coverage for this bucket.                                |
| `sourceEnd`                                                                   | *string*                                                                      | :heavy_check_mark:                                                            | Null means no source coverage for this bucket.                                |
| `requests`                                                                    | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `completed`                                                                   | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `aborted`                                                                     | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `probeExpired`                                                                | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `captureStopped`                                                              | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `unsupported`                                                                 | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `serverErrors`                                                                | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `responseWireBytes`                                                           | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `rps`                                                                         | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `p95`                                                                         | [operations.P95](../../models/operations/p95.md)                              | :heavy_check_mark:                                                            | Histogram percentile bounds in seconds; null means no completed observations. |
| `p99`                                                                         | [operations.P99](../../models/operations/p99.md)                              | :heavy_check_mark:                                                            | Histogram percentile bounds in seconds; null means no completed observations. |
| `ttfbP95`                                                                     | [operations.TtfbP95](../../models/operations/ttfbp95.md)                      | :heavy_check_mark:                                                            | Histogram percentile bounds in seconds; null means no completed observations. |
| `cpuPercent`                                                                  | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `memoryBytes`                                                                 | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `activeRequests`                                                              | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `cpuPeakPercent`                                                              | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `memoryPeakBytes`                                                             | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `activePeakRequests`                                                          | *number*                                                                      | :heavy_check_mark:                                                            | Null means no observed data, not zero.                                        |
| `statusCounts`                                                                | Record<string, *number*>                                                      | :heavy_check_mark:                                                            | N/A                                                                           |
| `availableReplicaSeconds`                                                     | *number*                                                                      | :heavy_check_mark:                                                            | N/A                                                                           |
| `unavailableReplicaSeconds`                                                   | *number*                                                                      | :heavy_check_mark:                                                            | N/A                                                                           |
| `captureQuality`                                                              | *number*[]                                                                    | :heavy_check_mark:                                                            | N/A                                                                           |