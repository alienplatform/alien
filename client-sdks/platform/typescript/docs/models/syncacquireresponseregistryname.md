# SyncAcquireResponseRegistryName

## Example Usage

```typescript
import { SyncAcquireResponseRegistryName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseRegistryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                              | [models.SyncAcquireResponseRegistryNameSecretRef](../models/syncacquireresponseregistrynamesecretref.md) | :heavy_check_mark:                                                                                       | Reference to a Kubernetes Secret                                                                         |