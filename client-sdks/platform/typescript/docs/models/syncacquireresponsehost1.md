# SyncAcquireResponseHost1

## Example Usage

```typescript
import { SyncAcquireResponseHost1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHost1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponseHostSecretRef1](../models/syncacquireresponsehostsecretref1.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |