# CreateDeploymentResponse

## Example Usage

```typescript
import { CreateDeploymentResponse } from "@alienplatform/manager-api/models";

let value: CreateDeploymentResponse = {
  deployment: {
    createdAt: "1717217133195",
    deploymentGroupId: "<id>",
    id: "<id>",
    name: "<value>",
    platform: "kubernetes",
    retryRequested: false,
    status: "<value>",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `deployment`                                                 | [models.DeploymentResponse](../models/deploymentresponse.md) | :heavy_check_mark:                                           | N/A                                                          |
| `token`                                                      | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |