# SyncAcquireResponseTableName2

## Example Usage

```typescript
import { SyncAcquireResponseTableName2 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTableName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncAcquireResponseTableNameSecretRef2](../models/syncacquireresponsetablenamesecretref2.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |