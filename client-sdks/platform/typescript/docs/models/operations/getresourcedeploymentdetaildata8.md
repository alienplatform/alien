# GetResourceDeploymentDetailData8

## Example Usage

```typescript
import { GetResourceDeploymentDetailData8 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailData8 = {
  keyId: "<id>",
  keyOperations: [
    "<value 1>",
  ],
  keyType: "<value>",
  status: {
    health: "unknown",
    lifecycle: "running",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `enabled`                                                                                                                | *boolean*                                                                                                                | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `keyId`                                                                                                                  | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `keyOperations`                                                                                                          | *string*[]                                                                                                               | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `keyType`                                                                                                                | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `recoveryLevel`                                                                                                          | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus72](../../models/operations/getresourcedeploymentdetaildatastatus72.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |