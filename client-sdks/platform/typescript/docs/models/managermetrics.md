# ManagerMetrics

Runtime metrics (self-reported via heartbeat)

## Example Usage

```typescript
import { ManagerMetrics } from "@alienplatform/platform-api/models";

let value: ManagerMetrics = {};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `activeDeployments`           | *number*                      | :heavy_minus_sign:            | Number of active deployments  |
| `pendingDeployments`          | *number*                      | :heavy_minus_sign:            | Number of pending deployments |
| `memoryUsageMb`               | *number*                      | :heavy_minus_sign:            | Memory usage in megabytes     |
| `cpuUsagePercent`             | *number*                      | :heavy_minus_sign:            | CPU usage percentage          |