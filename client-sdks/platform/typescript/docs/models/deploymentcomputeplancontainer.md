# DeploymentComputePlanContainer

## Example Usage

```typescript
import { DeploymentComputePlanContainer } from "@alienplatform/platform-api/models";

let value: DeploymentComputePlanContainer = {
  containerId: "<id>",
  choices: {},
  cpu: {
    min: "<value>",
    desired: "<value>",
  },
  memory: {
    min: "<value>",
    desired: "<value>",
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `containerId`                                                                  | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `choices`                                                                      | [models.Choices](../models/choices.md)                                         | :heavy_check_mark:                                                             | Release-declared choices for each container resource dimension.                |
| `cpu`                                                                          | [models.DeploymentComputePlanCpu](../models/deploymentcomputeplancpu.md)       | :heavy_check_mark:                                                             | N/A                                                                            |
| `memory`                                                                       | [models.DeploymentComputePlanMemory](../models/deploymentcomputeplanmemory.md) | :heavy_check_mark:                                                             | N/A                                                                            |