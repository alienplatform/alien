# CommandDeploymentInfo

## Example Usage

```typescript
import { CommandDeploymentInfo } from "@alienplatform/platform-api/models";

let value: CommandDeploymentInfo = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  name: "<value>",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        | Example                                                                            |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `id`                                                                               | *string*                                                                           | :heavy_check_mark:                                                                 | Unique identifier for the deployment.                                              | dep_0c29fq4a2yjb7kx3smwdgxlc                                                       |
| `name`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |                                                                                    |
| `deploymentGroup`                                                                  | [models.CommandDeploymentGroupInfo](../models/commanddeploymentgroupinfo.md)       | :heavy_minus_sign:                                                                 | N/A                                                                                |                                                                                    |
| `platform`                                                                         | [models.CommandDeploymentInfoPlatform](../models/commanddeploymentinfoplatform.md) | :heavy_minus_sign:                                                                 | Represents the target cloud platform.                                              |                                                                                    |
| `environmentInfo`                                                                  | *models.CommandDeploymentInfoEnvironmentInfoUnion*                                 | :heavy_minus_sign:                                                                 | Platform-specific environment information                                          |                                                                                    |
| `managerId`                                                                        | *string*                                                                           | :heavy_minus_sign:                                                                 | Manager ID for obtaining access tokens                                             |                                                                                    |
| `managerUrl`                                                                       | *string*                                                                           | :heavy_minus_sign:                                                                 | URL of the manager for direct payload access                                       |                                                                                    |
| `managerName`                                                                      | *string*                                                                           | :heavy_minus_sign:                                                                 | Human-readable name of the manager                                                 |                                                                                    |
| `managerIsSystem`                                                                  | *boolean*                                                                          | :heavy_minus_sign:                                                                 | Whether the manager is Alien-hosted (system)                                       |                                                                                    |