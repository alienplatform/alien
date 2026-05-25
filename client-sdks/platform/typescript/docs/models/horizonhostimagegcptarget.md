# HorizonHostImageGcpTarget

GCP Horizon host image catalog.

## Example Usage

```typescript
import { HorizonHostImageGcpTarget } from "@alienplatform/platform-api/models";

let value: HorizonHostImageGcpTarget = {
  images: {},
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `images`                                                                                             | Record<string, [models.SyncReconcileResponseGcpImages](../models/syncreconcileresponsegcpimages.md)> | :heavy_check_mark:                                                                                   | Images by architecture.                                                                              |