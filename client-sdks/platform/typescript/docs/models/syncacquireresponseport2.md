# SyncAcquireResponsePort2

## Example Usage

```typescript
import { SyncAcquireResponsePort2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePort2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponsePortSecretRef2](../models/syncacquireresponseportsecretref2.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |