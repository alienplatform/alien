# HorizonMachineImageAzureTarget

Azure Horizon machine image catalog.

## Example Usage

```typescript
import { HorizonMachineImageAzureTarget } from "@alienplatform/platform-api/models";

let value: HorizonMachineImageAzureTarget = {
  images: {
    "key": {
      imageVersionId: "<id>",
    },
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `images`                                                                                                 | Record<string, [models.SyncReconcileResponseAzureImages](../models/syncreconcileresponseazureimages.md)> | :heavy_check_mark:                                                                                       | Images by architecture.                                                                                  |