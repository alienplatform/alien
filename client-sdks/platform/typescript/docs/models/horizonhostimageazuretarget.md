# HorizonHostImageAzureTarget

Azure Horizon host image catalog.

## Example Usage

```typescript
import { HorizonHostImageAzureTarget } from "@alienplatform/platform-api/models";

let value: HorizonHostImageAzureTarget = {
  images: {
    "key": {
      imageDefinitionId: "<id>",
    },
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `images`                                                                                                 | Record<string, [models.SyncReconcileResponseAzureImages](../models/syncreconcileresponseazureimages.md)> | :heavy_check_mark:                                                                                       | Images by architecture.                                                                                  |