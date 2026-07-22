# DebugSessionDeployment

## Example Usage

```typescript
import { DebugSessionDeployment } from "@alienplatform/platform-api/models";

let value: DebugSessionDeployment = {
  id: "dep_0c29fq4a2yjb7kx3smwdgxlc",
  name: "<value>",
  deploymentGroup: {
    id: "dg_r27ict8c7vcgsumpj90ackf7b",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        | Example                                                                                            |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `id`                                                                                               | *string*                                                                                           | :heavy_check_mark:                                                                                 | Unique identifier for the deployment.                                                              | dep_0c29fq4a2yjb7kx3smwdgxlc                                                                       |
| `name`                                                                                             | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |                                                                                                    |
| `deploymentGroup`                                                                                  | [models.DebugSessionDeploymentDeploymentGroup](../models/debugsessiondeploymentdeploymentgroup.md) | :heavy_minus_sign:                                                                                 | N/A                                                                                                |                                                                                                    |
| `platform`                                                                                         | [models.DebugSessionDeploymentPlatform](../models/debugsessiondeploymentplatform.md)               | :heavy_minus_sign:                                                                                 | Represents the target cloud platform.                                                              |                                                                                                    |
| `environmentInfo`                                                                                  | *models.DebugSessionDeploymentEnvironmentInfoUnion*                                                | :heavy_minus_sign:                                                                                 | Platform-specific environment information                                                          |                                                                                                    |
