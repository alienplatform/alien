# SyncAcquireResponseResourceGroupName1

## Example Usage

```typescript
import { SyncAcquireResponseResourceGroupName1 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseResourceGroupName1 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncAcquireResponseResourceGroupNameSecretRef1](../models/syncacquireresponseresourcegroupnamesecretref1.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |