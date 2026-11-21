# GetResourceDeploymentDetailDataData

## Example Usage

```typescript
import { GetResourceDeploymentDetailDataData } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailDataData = {
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `details`                                                                                                                | Record<string, *any*>                                                                                                    | :heavy_minus_sign:                                                                                                       | Type-specific fields the extension's controller reports, which Alien does not read.                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus82](../../models/operations/getresourcedeploymentdetaildatastatus82.md) | :heavy_check_mark:                                                                                                       | The status fields every heartbeat carries, so a custom heartbeat's health reads like a<br/>built-in one's.               |