# GetResourceDeploymentDetailData7

## Example Usage

```typescript
import { GetResourceDeploymentDetailData7 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailData7 = {
  cryptoKeyName: "<value>",
  purpose: "<value>",
  status: {
    health: "degraded",
    lifecycle: "updating",
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `algorithm`                                                                                                              | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `cryptoKeyName`                                                                                                          | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `primaryState`                                                                                                           | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `primaryVersion`                                                                                                         | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `purpose`                                                                                                                | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus71](../../models/operations/getresourcedeploymentdetaildatastatus71.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |