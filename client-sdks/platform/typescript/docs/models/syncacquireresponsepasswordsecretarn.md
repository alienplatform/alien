# SyncAcquireResponsePasswordSecretArn

## Example Usage

```typescript
import { SyncAcquireResponsePasswordSecretArn } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponsePasswordSecretArn = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                        | [models.SyncAcquireResponsePasswordSecretArnSecretRef](../models/syncacquireresponsepasswordsecretarnsecretref.md) | :heavy_check_mark:                                                                                                 | Reference to a Kubernetes Secret                                                                                   |