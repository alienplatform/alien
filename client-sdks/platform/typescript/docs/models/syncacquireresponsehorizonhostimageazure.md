# SyncAcquireResponseHorizonHostImageAzure

Azure Horizon host image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseHorizonHostImageAzure } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizonHostImageAzure = {
  images: {
    "key": {
      imageDefinitionId: "<id>",
    },
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `images`                                                                                             | Record<string, [models.SyncAcquireResponseAzureImages](../models/syncacquireresponseazureimages.md)> | :heavy_check_mark:                                                                                   | Images by architecture.                                                                              |