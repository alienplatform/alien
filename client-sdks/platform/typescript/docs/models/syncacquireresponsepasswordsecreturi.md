# SyncAcquireResponsePasswordSecretUri

## Example Usage

```typescript
import { SyncAcquireResponsePasswordSecretUri } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePasswordSecretUri = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                        | [models.SyncAcquireResponsePasswordSecretUriSecretRef](../models/syncacquireresponsepasswordsecreturisecretref.md) | :heavy_check_mark:                                                                                                 | Reference to a Kubernetes Secret                                                                                   |