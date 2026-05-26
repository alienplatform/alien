# SyncAcquireResponseHorizonMachineImageAzure

Azure Horizon machine image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseHorizonMachineImageAzure } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizonMachineImageAzure = {
  images: {},
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `images`                                                                                             | Record<string, [models.SyncAcquireResponseAzureImages](../models/syncacquireresponseazureimages.md)> | :heavy_check_mark:                                                                                   | Images by architecture.                                                                              |