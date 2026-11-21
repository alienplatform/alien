# DataLocal12

## Example Usage

```typescript
import { DataLocal12 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal12 = {
  reachable: true,
  registryUrl: "https://instructive-scratch.org/",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `reachable`                                                                                                              | *boolean*                                                                                                                | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `registryUrl`                                                                                                            | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus59](../../models/operations/getresourcedeploymentdetaildatastatus59.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `backend`                                                                                                                | *"local"*                                                                                                                | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |