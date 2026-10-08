# StoragePath

## Example Usage

```typescript
import { StoragePath } from "@alienplatform/platform-api/models";

let value: StoragePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `secretRef`                                                      | [models.StoragePathSecretRef](../models/storagepathsecretref.md) | :heavy_check_mark:                                               | Reference to a Kubernetes Secret                                 |
