# HorizonMachineImageAzureConfig

Azure Horizon machine image catalog.

## Example Usage

```typescript
import { HorizonMachineImageAzureConfig } from "@alienplatform/platform-api/models";

let value: HorizonMachineImageAzureConfig = {
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
| `images`                                                                                       | Record<string, [models.TargetDeploymentAzureImages](../models/targetdeploymentazureimages.md)> | :heavy_check_mark:                                                                             | Images by architecture.                                                                        |