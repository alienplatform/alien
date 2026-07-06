# SyncAcquireResponseHost2

## Example Usage

```typescript
import { SyncAcquireResponseHost2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHost2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                | [models.SyncAcquireResponseHostSecretRef2](../models/syncacquireresponsehostsecretref2.md) | :heavy_check_mark:                                                                         | Reference to a Kubernetes Secret                                                           |