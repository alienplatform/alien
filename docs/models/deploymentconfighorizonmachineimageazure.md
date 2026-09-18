# DeploymentConfigHorizonMachineImageAzure

Azure Horizon machine image catalog.

## Example Usage

```typescript
import { DeploymentConfigHorizonMachineImageAzure } from "@alienplatform/platform-api/models";

let value: DeploymentConfigHorizonMachineImageAzure = {
  images: {
    "key": {
      imageVersionId: "<id>",
    },
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `images`                                                                                       | Record<string, [models.DeploymentConfigAzureImages](../models/deploymentconfigazureimages.md)> | :heavy_check_mark:                                                                             | Images by architecture.                                                                        |