# SyncAcquireResponseDeploymentPendingPreparedStackManagement1

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentPendingPreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentPendingPreparedStackManagement1 = {
  extend: {
    "key": [],
    "key1": [
      {
        description: "properly with suddenly",
        id: "<id>",
        platforms: {},
      },
    ],
    "key2": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.SyncAcquireResponseDeploymentPendingPreparedStackExtendUnion*[]>                                           | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
