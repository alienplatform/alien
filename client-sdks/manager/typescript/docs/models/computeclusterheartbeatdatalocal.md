# ComputeClusterHeartbeatDataLocal

## Example Usage

```typescript
import { ComputeClusterHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: ComputeClusterHeartbeatDataLocal = {
  dockerAvailable: true,
  name: "<value>",
  networkAvailable: true,
  nodes: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `dockerApiVersion`                                                                 | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `dockerArch`                                                                       | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `dockerAvailable`                                                                  | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `dockerOs`                                                                         | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `dockerVersion`                                                                    | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `hostIdentifier`                                                                   | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `name`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `networkAvailable`                                                                 | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `networkName`                                                                      | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `nodes`                                                                            | [models.ObservedCounts](../models/observedcounts.md)                               | :heavy_check_mark:                                                                 | N/A                                                                                |
| `runningContainers`                                                                | *number*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.ComputeClusterHeartbeatStatus](../models/computeclusterheartbeatstatus.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `trackedContainers`                                                                | *number*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `backend`                                                                          | *"local"*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |