# GetResourceDeploymentDetailData9

## Example Usage

```typescript
import { GetResourceDeploymentDetailData9 } from "@alienplatform/platform-api/models/operations";

let value: GetResourceDeploymentDetailData9 = {
  data: {
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "failed",
      partial: false,
      stale: false,
    },
  },
  resourceType: "<value>",
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `data`                                                                                                           | [operations.GetResourceDeploymentDetailDataData](../../models/operations/getresourcedeploymentdetaildatadata.md) | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `resourceType`                                                                                                   | *string*                                                                                                         | :heavy_check_mark:                                                                                               | The type of a resource an extension defines, such as `acme/dns-zone`.                                            |