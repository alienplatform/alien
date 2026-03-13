# SyncAcquireResponseNamespace2

## Example Usage

```typescript
import { SyncAcquireResponseNamespace2 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseNamespace2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncAcquireResponseNamespaceSecretRef2](../models/syncacquireresponsenamespacesecretref2.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |