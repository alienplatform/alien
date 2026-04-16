# SyncAcquireResponseCollectionName

## Example Usage

```typescript
import { SyncAcquireResponseCollectionName } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCollectionName = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                                  | [models.SyncAcquireResponseCollectionNameSecretRef](../models/syncacquireresponsecollectionnamesecretref.md) | :heavy_check_mark:                                                                                           | Reference to a Kubernetes Secret                                                                             |