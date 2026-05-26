# HorizonMachineImageGcpTarget

GCP Horizon machine image catalog.

## Example Usage

```typescript
import { HorizonMachineImageGcpTarget } from "@alienplatform/platform-api/models";

let value: HorizonMachineImageGcpTarget = {
  images: {},
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `images`                                                                                             | Record<string, [models.SyncReconcileResponseGcpImages](../models/syncreconcileresponsegcpimages.md)> | :heavy_check_mark:                                                                                   | Images by architecture.                                                                              |