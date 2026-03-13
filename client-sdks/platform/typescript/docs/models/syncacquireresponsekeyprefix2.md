# SyncAcquireResponseKeyPrefix2

## Example Usage

```typescript
import { SyncAcquireResponseKeyPrefix2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseKeyPrefix2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncAcquireResponseKeyPrefixSecretRef2](../models/syncacquireresponsekeyprefixsecretref2.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |