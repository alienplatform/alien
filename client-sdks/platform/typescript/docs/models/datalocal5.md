# DataLocal5

## Example Usage

```typescript
import { DataLocal5 } from "@alienplatform/platform-api/models";

let value: DataLocal5 = {
  dockerAvailable: false,
  name: "<value>",
  networkAvailable: false,
  nodes: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `dockerApiVersion`                                                         | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `dockerArch`                                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `dockerAvailable`                                                          | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `dockerOs`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `dockerVersion`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `hostIdentifier`                                                           | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `networkAvailable`                                                         | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `networkName`                                                              | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `nodes`                                                                    | [models.Nodes5](../models/nodes5.md)                                       | :heavy_check_mark:                                                         | N/A                                                                        |
| `runningContainers`                                                        | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus23](../models/resourceheartbeatstatus23.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `trackedContainers`                                                        | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `backend`                                                                  | *"local"*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |