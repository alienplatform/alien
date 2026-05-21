# DeploymentInfo

## Example Usage

```typescript
import { DeploymentInfo } from "@alienplatform/platform-api/models";

let value: DeploymentInfo = {
  tokenType: "deployment-group",
  deploymentGroup: {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    name: "<value>",
  },
  workspace: {
    id: "<id>",
    name: "<value>",
  },
  project: {
    name: "<value>",
    portal: {
      appearance: {},
    },
  },
  packages: {
    ready: true,
  },
  installContext: {
    targets: {
      "key": {
        platform: "kubernetes",
        managerUrl: "https://excitable-drug.name/",
        managementConfig: {
          managingTenantId: "<id>",
          platform: "azure",
        },
      },
    },
  },
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `tokenType`                                                                        | [models.DeploymentInfoTokenType](../models/deploymentinfotokentype.md)             | :heavy_check_mark:                                                                 | Type of token used to authenticate this request                                    |
| `deployment`                                                                       | [models.DeploymentInfoDeployment](../models/deploymentinfodeployment.md)           | :heavy_minus_sign:                                                                 | Deployment details (present when using a deployment-scoped token)                  |
| `deploymentGroup`                                                                  | [models.DeploymentInfoDeploymentGroup](../models/deploymentinfodeploymentgroup.md) | :heavy_minus_sign:                                                                 | Deployment group details (present when using a deployment-group token)             |
| `workspace`                                                                        | [models.DeploymentInfoWorkspace](../models/deploymentinfoworkspace.md)             | :heavy_check_mark:                                                                 | N/A                                                                                |
| `project`                                                                          | [models.DeploymentInfoProject](../models/deploymentinfoproject.md)                 | :heavy_check_mark:                                                                 | N/A                                                                                |
| `packages`                                                                         | [models.Packages](../models/packages.md)                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `installContext`                                                                   | [models.DeploymentInfoInstallContext](../models/deploymentinfoinstallcontext.md)   | :heavy_check_mark:                                                                 | N/A                                                                                |