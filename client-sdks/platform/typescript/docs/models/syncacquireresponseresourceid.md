# SyncAcquireResponseResourceId

## Example Usage

```typescript
import { SyncAcquireResponseResourceId } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseResourceId = {
  secretRef: {
    key: "<key>",
    name: "<value>",
  },
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `secretRef`                                                                                          | [models.SyncAcquireResponseResourceIdSecretRef](../models/syncacquireresponseresourceidsecretref.md) | :heavy_check_mark:                                                                                   | Reference to a Kubernetes Secret                                                                     |