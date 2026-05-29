# DataLocal5

## Example Usage

```typescript
import { DataLocal5 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal5 = {
  dockerAvailable: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-09-24T00:42:05.626Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  networkAvailable: false,
  nodes: {},
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `dockerApiVersion`                                                                                       | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `dockerArch`                                                                                             | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `dockerAvailable`                                                                                        | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `dockerOs`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `dockerVersion`                                                                                          | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent21](../../models/operations/getrawresourceheartbeatevent21.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `hostIdentifier`                                                                                         | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `name`                                                                                                   | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `networkAvailable`                                                                                       | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `networkName`                                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `nodes`                                                                                                  | [operations.Nodes4](../../models/operations/nodes4.md)                                                   | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `runningContainers`                                                                                      | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus21](../../models/operations/datastatus21.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `trackedContainers`                                                                                      | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"local"*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |