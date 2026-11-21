# GetResourceDeploymentDetailData2

## Example Usage

```typescript
import { GetResourceDeploymentDetailData2 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailData2 = {
  managedTags: {},
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `location`                                                                                                               | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `managedTags`                                                                                                            | Record<string, *string*>                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `name`                                                                                                                   | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `provisioningState`                                                                                                      | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `resourceId`                                                                                                             | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus66](../../models/operations/getresourcedeploymentdetaildatastatus66.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |