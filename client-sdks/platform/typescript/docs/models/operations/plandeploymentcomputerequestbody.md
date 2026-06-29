# PlanDeploymentComputeRequestBody

## Example Usage

```typescript
import { PlanDeploymentComputeRequestBody } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeRequestBody = {
  platform: "azure",
  setupMethod: "terraform",
  stackSettings: {},
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `platform`                                                                                                     | [operations.PlanDeploymentComputePlatform](../../models/operations/plandeploymentcomputeplatform.md)           | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `setupMethod`                                                                                                  | [models.DeploymentSetupMethod](../../models/deploymentsetupmethod.md)                                          | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `region`                                                                                                       | *string*                                                                                                       | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |
| `stackSettings`                                                                                                | [operations.PlanDeploymentComputeStackSettings](../../models/operations/plandeploymentcomputestacksettings.md) | :heavy_check_mark:                                                                                             | N/A                                                                                                            |