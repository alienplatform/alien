# Point

## Example Usage

```typescript
import { Point } from "@alienplatform/platform-api/models/operations";

let value: Point = {
  timestamp: "<value>",
  sample: {
    observedAt: "<value>",
    sourceAt: "<value>",
    metrics: {
      cpuPercent: 3550.19,
      memoryBytes: 9309.67,
      memoryPercent: 2550.95,
      inFlightRequests: 5074.25,
      requestsPerSecond: 5155.04,
      status2xxPerSecond: 8678.59,
      status4xxPerSecond: 3280.2,
      status5xxPerSecond: null,
      latencyP95Ms: 5785.86,
      latencyP99Ms: 245.59,
    },
    payload: "<value>",
  },
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `timestamp`                                            | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `sample`                                               | [operations.Sample](../../models/operations/sample.md) | :heavy_check_mark:                                     | N/A                                                    |