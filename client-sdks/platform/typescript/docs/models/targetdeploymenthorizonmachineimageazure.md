# TargetDeploymentHorizonMachineImageAzure

Azure machine image catalog.

## Example Usage

```typescript
import { TargetDeploymentHorizonMachineImageAzure } from "@alienplatform/platform-api/models";

let value: TargetDeploymentHorizonMachineImageAzure = {
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