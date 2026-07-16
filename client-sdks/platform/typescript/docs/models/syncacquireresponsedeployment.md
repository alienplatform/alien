# SyncAcquireResponseDeployment

## Example Usage

```typescript
import { SyncAcquireResponseDeployment } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeployment = {
  deploymentId: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  projectId: "<id>",
  deploymentGroupId: "dg_r27ict8c7vcgsumpj90ackf7b",
  current: {
    platform: "aws",
    protocolVersion: 626709,
    status: "initial-setup",
  },
  config: {
    environmentVariables: {
      createdAt: "1708829691352",
      hash: "<value>",
      variables: [
        {
          name: "<value>",
          type: "plain",
          value: "<value>",
        },
      ],
    },
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      | Example                                                                                          |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `deploymentId`                                                                                   | *string*                                                                                         | :heavy_check_mark:                                                                               | ID of the deployment                                                                             | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                     |
| `projectId`                                                                                      | *string*                                                                                         | :heavy_check_mark:                                                                               | Project ID the deployment belongs to                                                             |                                                                                                  |
| `deploymentGroupId`                                                                              | *string*                                                                                         | :heavy_check_mark:                                                                               | Deployment group ID the deployment belongs to                                                    | dg_r27ict8c7vcgsumpj90ackf7b                                                                     |
| `setupMethod`                                                                                    | [models.DeploymentSetupMethod](../models/deploymentsetupmethod.md)                               | :heavy_minus_sign:                                                                               | N/A                                                                                              |                                                                                                  |
| `current`                                                                                        | [models.SyncAcquireResponseDeploymentCurrent](../models/syncacquireresponsedeploymentcurrent.md) | :heavy_check_mark:                                                                               | Current deployment state (includes releases)                                                     |                                                                                                  |
| `config`                                                                                         | [models.SyncAcquireResponseDeploymentConfig](../models/syncacquireresponsedeploymentconfig.md)   | :heavy_check_mark:                                                                               | Deployment configuration                                                                         |                                                                                                  |