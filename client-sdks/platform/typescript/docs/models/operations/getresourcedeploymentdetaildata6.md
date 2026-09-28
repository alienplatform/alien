# GetResourceDeploymentDetailData6

## Example Usage

```typescript
import { GetResourceDeploymentDetailData6 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailData6 = {
  enabled: true,
  keyArn: "<value>",
  keySpec: "<value>",
  keyState: "<value>",
  keyUsage: "<value>",
  status: {
    health: "unknown",
    lifecycle: "unknown",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `enabled`                                                                                                                | *boolean*                                                                                                                | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `keyArn`                                                                                                                 | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `keySpec`                                                                                                                | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `keyState`                                                                                                               | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `keyUsage`                                                                                                               | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus70](../../models/operations/getresourcedeploymentdetaildatastatus70.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |