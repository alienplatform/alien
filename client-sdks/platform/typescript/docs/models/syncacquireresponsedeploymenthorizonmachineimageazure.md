# SyncAcquireResponseDeploymentHorizonMachineImageAzure

Azure Horizon machine image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentHorizonMachineImageAzure } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentHorizonMachineImageAzure = {
  images: {
    "key": {
      imageVersionId: "<id>",
    },
  },
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `images`                                                                                                                 | Record<string, [models.SyncAcquireResponseDeploymentAzureImages](../models/syncacquireresponsedeploymentazureimages.md)> | :heavy_check_mark:                                                                                                       | Images by architecture.                                                                                                  |