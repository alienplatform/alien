# DeploymentGroupScope

Deployment group-scoped configuration

## Example Usage

```typescript
import { DeploymentGroupScope } from "@aliendotdev/platform-api/models";

let value: DeploymentGroupScope = {
  type: "deployment-group",
  deploymentGroupId: "<id>",
  projectId: "<id>",
  role: "deployment-group.deployer",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    | Example                                                        |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `type`                                                         | *"deployment-group"*                                           | :heavy_check_mark:                                             | N/A                                                            |                                                                |
| `deploymentGroupId`                                            | *string*                                                       | :heavy_check_mark:                                             | ID of the deployment group this is scoped to                   |                                                                |
| `projectId`                                                    | *string*                                                       | :heavy_check_mark:                                             | ID of the project this deployment group belongs to             |                                                                |
| `role`                                                         | [models.DeploymentGroupRole](../models/deploymentgrouprole.md) | :heavy_check_mark:                                             | Role for deployment group-scoped service accounts              | workspace.member                                               |