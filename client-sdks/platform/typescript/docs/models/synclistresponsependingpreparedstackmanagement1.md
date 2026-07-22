# SyncListResponsePendingPreparedStackManagement1

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackManagement1 = {
  extend: {
    "key": [
      {
        description: "alongside however generously ick teammate for",
        id: "<id>",
        platforms: {},
      },
    ],
    "key1": [
      "<value>",
    ],
    "key2": [
      {
        description: "alongside however generously ick teammate for",
        id: "<id>",
        platforms: {},
      },
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.SyncListResponsePendingPreparedStackExtendUnion*[]>                                                        | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
