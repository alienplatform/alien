# SyncAcquireResponseDefaultDomain

## Example Usage

```typescript
import { SyncAcquireResponseDefaultDomain } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDefaultDomain = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                | [models.SyncAcquireResponseDefaultDomainSecretRef](../models/syncacquireresponsedefaultdomainsecretref.md) | :heavy_check_mark:                                                                                         | Reference to a Kubernetes Secret                                                                           |