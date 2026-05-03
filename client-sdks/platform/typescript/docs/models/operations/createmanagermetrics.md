# CreateManagerMetrics

Runtime metrics (self-reported via heartbeat)

## Example Usage

```typescript
import { CreateManagerMetrics } from "@alienplatform/platform-api/models/operations";

let value: CreateManagerMetrics = {};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `activeDeployments`           | *number*                      | :heavy_minus_sign:            | Number of active deployments  |
| `pendingDeployments`          | *number*                      | :heavy_minus_sign:            | Number of pending deployments |
| `memoryUsageMb`               | *number*                      | :heavy_minus_sign:            | Memory usage in megabytes     |
| `cpuUsagePercent`             | *number*                      | :heavy_minus_sign:            | CPU usage percentage          |