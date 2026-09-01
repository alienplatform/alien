# PlanDeploymentComputeRequest

## Example Usage

```typescript
import { PlanDeploymentComputeRequest } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeRequest = {
  platform: "azure",
  setupMethod: "helm",
  stackSettings: {},
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `setupItem`                                                                                                    | [operations.PlanDeploymentComputeSetupItem](../../models/operations/plandeploymentcomputesetupitem.md)         | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |
| `platform`                                                                                                     | [operations.PlanDeploymentComputePlatform](../../models/operations/plandeploymentcomputeplatform.md)           | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `setupMethod`                                                                                                  | [models.DeploymentSetupMethod](../../models/deploymentsetupmethod.md)                                          | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `region`                                                                                                       | *string*                                                                                                       | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |
| `stackSettings`                                                                                                | [operations.PlanDeploymentComputeStackSettings](../../models/operations/plandeploymentcomputestacksettings.md) | :heavy_check_mark:                                                                                             | N/A                                                                                                            |