# SyncAcquireResponseResourceGroupName3

## Example Usage

```typescript
import { SyncAcquireResponseResourceGroupName3 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseResourceGroupName3 = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                                          | [models.SyncAcquireResponseResourceGroupNameSecretRef3](../models/syncacquireresponseresourcegroupnamesecretref3.md) | :heavy_check_mark:                                                                                                   | Reference to a Kubernetes Secret                                                                                     |