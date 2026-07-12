# CreateDeploymentResponse

## Example Usage

```typescript
import { CreateDeploymentResponse } from "@alienplatform/manager-api/models";

let value: CreateDeploymentResponse = {
  deployment: {
    createdAt: "1717217133195",
    deploymentGroupId: "<id>",
    deploymentProtocolVersion: 555963,
    id: "<id>",
    name: "<value>",
    platform: "test",
    projectId: "<id>",
    retryRequested: true,
    status: "<value>",
    workspaceId: "<id>",
  },
  deploymentModel: "pull",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `deployment`                                                           | [models.DeploymentResponse](../models/deploymentresponse.md)           | :heavy_check_mark:                                                     | N/A                                                                    |
| `deploymentModel`                                                      | [models.DeploymentModel](../models/deploymentmodel.md)                 | :heavy_check_mark:                                                     | Deployment model: how updates are delivered to the remote environment. |
| `token`                                                                | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |