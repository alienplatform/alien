# ContainerHeartbeatDataLocal

## Example Usage

```typescript
import { ContainerHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: ContainerHeartbeatDataLocal = {
  bindMountCount: 916242,
  events: [],
  portCount: 776553,
  runtimeReachable: false,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `bindMountCount`                                                       | *number*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `containerId`                                                          | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `cpu`                                                                  | [models.MetricSample](../models/metricsample.md)                       | :heavy_minus_sign:                                                     | N/A                                                                    |
| `events`                                                               | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                 | :heavy_check_mark:                                                     | N/A                                                                    |
| `image`                                                                | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `localUrl`                                                             | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `memory`                                                               | [models.MetricSample](../models/metricsample.md)                       | :heavy_minus_sign:                                                     | N/A                                                                    |
| `name`                                                                 | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `portCount`                                                            | *number*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `restartCount`                                                         | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `runtimeReachable`                                                     | *boolean*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |
| `runtimeStatus`                                                        | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `status`                                                               | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `backend`                                                              | *"local"*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |