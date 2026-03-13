# SyncAcquireResponseTableName1

## Example Usage

```typescript
import { SyncAcquireResponseTableName1 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseTableName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncAcquireResponseTableNameSecretRef1](../models/syncacquireresponsetablenamesecretref1.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |