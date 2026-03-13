# SyncAcquireResponseStoragePath

## Example Usage

```typescript
import { SyncAcquireResponseStoragePath } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseStoragePath = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `secretRef`                                                                                            | [models.SyncAcquireResponseStoragePathSecretRef](../models/syncacquireresponsestoragepathsecretref.md) | :heavy_check_mark:                                                                                     | Reference to a Kubernetes Secret                                                                       |