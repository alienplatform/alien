# DataLocal5

## Example Usage

```typescript
import { DataLocal5 } from "@alienplatform/platform-api/models/operations";

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
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopped",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `dockerApiVersion`                                                                                                       | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `dockerArch`                                                                                                             | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `dockerAvailable`                                                                                                        | *boolean*                                                                                                                | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `dockerOs`                                                                                                               | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `dockerVersion`                                                                                                          | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `hostIdentifier`                                                                                                         | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `name`                                                                                                                   | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `networkAvailable`                                                                                                       | *boolean*                                                                                                                | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `networkName`                                                                                                            | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `nodes`                                                                                                                  | [operations.Nodes5](../../models/operations/nodes5.md)                                                                   | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `runningContainers`                                                                                                      | *number*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus23](../../models/operations/getresourcedeploymentdetaildatastatus23.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `trackedContainers`                                                                                                      | *number*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `backend`                                                                                                                | *"local"*                                                                                                                | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |