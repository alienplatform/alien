# SyncAcquireResponseResourceGroupName2

## Example Usage

```typescript
import { SyncAcquireResponseResourceGroupName2 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseResourceGroupName2 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncAcquireResponseResourceGroupNameSecretRef2](../models/syncacquireresponseresourcegroupnamesecretref2.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |