# SyncAcquireResponseConnectionUrl

## Example Usage

```typescript
import { SyncAcquireResponseConnectionUrl } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseConnectionUrl = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncAcquireResponseConnectionUrlSecretRef](../models/syncacquireresponseconnectionurlsecretref.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |