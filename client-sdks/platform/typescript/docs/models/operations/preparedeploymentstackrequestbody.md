# PrepareDeploymentStackRequestBody

## Example Usage

```typescript
import { PrepareDeploymentStackRequestBody } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackRequestBody = {
  platform: "aws",
  setupMethod: "helm",
  stackSettings: {},
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `platform`                                                                                                       | [operations.PrepareDeploymentStackPlatform](../../models/operations/preparedeploymentstackplatform.md)           | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `setupMethod`                                                                                                    | [models.DeploymentSetupMethod](../../models/deploymentsetupmethod.md)                                            | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `region`                                                                                                         | *string*                                                                                                         | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `stackSettings`                                                                                                  | [operations.PrepareDeploymentStackStackSettings](../../models/operations/preparedeploymentstackstacksettings.md) | :heavy_check_mark:                                                                                               | N/A                                                                                                              |