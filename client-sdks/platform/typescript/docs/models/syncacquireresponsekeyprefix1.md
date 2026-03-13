# SyncAcquireResponseKeyPrefix1

## Example Usage

```typescript
import { SyncAcquireResponseKeyPrefix1 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseKeyPrefix1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncAcquireResponseKeyPrefixSecretRef1](../models/syncacquireresponsekeyprefixsecretref1.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |