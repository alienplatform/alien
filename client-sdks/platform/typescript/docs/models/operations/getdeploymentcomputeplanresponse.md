# GetDeploymentComputePlanResponse

The compute plan for the current settings; plan is absent when the deployment has no compute to plan (not on a cloud, no target release) or its manager predates planning.

## Example Usage

```typescript
import { GetDeploymentComputePlanResponse } from "@alienplatform/platform-api/models/operations";

let value: GetDeploymentComputePlanResponse = {};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `plan`                                                                | [models.DeploymentComputePlan](../../models/deploymentcomputeplan.md) | :heavy_minus_sign:                                                    | N/A                                                                   |