# PrepareDeploymentStackRequest

## Example Usage

```typescript
import { PrepareDeploymentStackRequest } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackRequest = {
  platform: "gcp",
  setupMethod: "cloudformation",
  stackSettings: {},
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `setupItem`                                                                                                      | [operations.PrepareDeploymentStackSetupItem](../../models/operations/preparedeploymentstacksetupitem.md)         | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `platform`                                                                                                       | [operations.PrepareDeploymentStackPlatform](../../models/operations/preparedeploymentstackplatform.md)           | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `setupMethod`                                                                                                    | [models.DeploymentSetupMethod](../../models/deploymentsetupmethod.md)                                            | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `region`                                                                                                         | *string*                                                                                                         | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `stackSettings`                                                                                                  | [operations.PrepareDeploymentStackStackSettings](../../models/operations/preparedeploymentstackstacksettings.md) | :heavy_check_mark:                                                                                               | N/A                                                                                                              |