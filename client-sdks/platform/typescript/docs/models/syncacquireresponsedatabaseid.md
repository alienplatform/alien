# SyncAcquireResponseDatabaseId

## Example Usage

```typescript
import { SyncAcquireResponseDatabaseId } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDatabaseId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncAcquireResponseDatabaseIdSecretRef](../models/syncacquireresponsedatabaseidsecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |