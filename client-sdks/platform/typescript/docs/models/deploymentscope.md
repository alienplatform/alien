# DeploymentScope

Deployment-scoped configuration

## Example Usage

```typescript
import { DeploymentScope } from "@alienplatform/platform-api/models";

let value: DeploymentScope = {
  type: "deployment",
  deploymentId: "<id>",
  projectId: "<id>",
  role: "deployment.viewer",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          | Example                                              |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `type`                                               | *"deployment"*                                       | :heavy_check_mark:                                   | N/A                                                  |                                                      |
| `deploymentId`                                       | *string*                                             | :heavy_check_mark:                                   | ID of the deployment this is scoped to               |                                                      |
| `projectId`                                          | *string*                                             | :heavy_check_mark:                                   | ID of the project this deployment belongs to         |                                                      |
| `role`                                               | [models.DeploymentRole](../models/deploymentrole.md) | :heavy_check_mark:                                   | Role for deployment-scoped service accounts          | workspace.member                                     |