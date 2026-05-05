# SyncAcquireResponseRepositoryName

## Example Usage

```typescript
import { SyncAcquireResponseRepositoryName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseRepositoryName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.SyncAcquireResponseRepositoryNameSecretRef](../models/syncacquireresponserepositorynamesecretref.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |