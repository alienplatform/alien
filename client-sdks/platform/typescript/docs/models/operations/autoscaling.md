# Autoscaling

## Example Usage

```typescript
import { Autoscaling } from "@aliendotdev/platform-api/models/operations";

let value: Autoscaling = {
  min: 387400,
  desired: 775471,
  max: 51523,
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `min`                          | *number*                       | :heavy_check_mark:             | N/A                            |
| `desired`                      | *number*                       | :heavy_check_mark:             | N/A                            |
| `max`                          | *number*                       | :heavy_check_mark:             | N/A                            |
| `targetCpuPercent`             | *number*                       | :heavy_minus_sign:             | N/A                            |
| `targetMemoryPercent`          | *number*                       | :heavy_minus_sign:             | N/A                            |
| `targetHttpInFlightPerReplica` | *number*                       | :heavy_minus_sign:             | N/A                            |
| `maxHttpP95LatencyMs`          | *number*                       | :heavy_minus_sign:             | N/A                            |