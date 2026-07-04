# SyncAcquireResponsePort3

## Example Usage

```typescript
import { SyncAcquireResponsePort3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePort3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponsePortSecretRef3](../models/syncacquireresponseportsecretref3.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |