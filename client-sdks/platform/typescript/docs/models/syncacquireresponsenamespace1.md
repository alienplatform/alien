# SyncAcquireResponseNamespace1

## Example Usage

```typescript
import { SyncAcquireResponseNamespace1 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseNamespace1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncAcquireResponseNamespaceSecretRef1](../models/syncacquireresponsenamespacesecretref1.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |