# GetContainerAttentionDeployment

## Example Usage

```typescript
import { GetContainerAttentionDeployment } from "@alienplatform/platform-api/models/operations";

let value: GetContainerAttentionDeployment = {
  clusterId: "<id>",
  deploymentId: "<id>",
  deploymentName: "<value>",
  issues: [
    {
      type: "unhealthy_machine",
      message: "<value>",
    },
  ],
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `clusterId`                                            | *string*                                               | :heavy_check_mark:                                     | N/A                                                    |
| `deploymentId`                                         | *string*                                               | :heavy_check_mark:                                     | Deployment ID for linking to the deployment            |
| `deploymentName`                                       | *string*                                               | :heavy_check_mark:                                     | Deployment name for display                            |
| `deploymentGroupId`                                    | *string*                                               | :heavy_minus_sign:                                     | Deployment group ID for linking                        |
| `deploymentGroupName`                                  | *string*                                               | :heavy_minus_sign:                                     | N/A                                                    |
| `projectName`                                          | *string*                                               | :heavy_minus_sign:                                     | N/A                                                    |
| `issues`                                               | [operations.Issue](../../models/operations/issue.md)[] | :heavy_check_mark:                                     | N/A                                                    |