# SyncAcquireResponseHorizonMachineImageGcp

GCP Horizon machine image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseHorizonMachineImageGcp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizonMachineImageGcp = {
  images: {},
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `images`                                                                                         | Record<string, [models.SyncAcquireResponseGcpImages](../models/syncacquireresponsegcpimages.md)> | :heavy_check_mark:                                                                               | Images by architecture.                                                                          |