# SyncAcquireResponseHorizonHostImageGcp

GCP Horizon host image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseHorizonHostImageGcp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizonHostImageGcp = {
  images: {
    "key": {
      sourceImage: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `images`                                                                                         | Record<string, [models.SyncAcquireResponseGcpImages](../models/syncacquireresponsegcpimages.md)> | :heavy_check_mark:                                                                               | Images by architecture.                                                                          |