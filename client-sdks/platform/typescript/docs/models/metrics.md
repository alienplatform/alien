# Metrics

Optional runtime metrics

## Example Usage

```typescript
import { Metrics } from "@aliendotdev/platform-api/models";

let value: Metrics = {};
```

## Fields

| Field                | Type                 | Required             | Description          |
| -------------------- | -------------------- | -------------------- | -------------------- |
| `activeDeployments`  | *number*             | :heavy_minus_sign:   | N/A                  |
| `pendingDeployments` | *number*             | :heavy_minus_sign:   | N/A                  |
| `memoryUsageMb`      | *number*             | :heavy_minus_sign:   | N/A                  |
| `cpuUsagePercent`    | *number*             | :heavy_minus_sign:   | N/A                  |