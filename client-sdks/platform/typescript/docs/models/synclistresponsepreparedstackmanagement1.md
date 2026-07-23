# SyncListResponsePreparedStackManagement1

## Example Usage

```typescript
import { SyncListResponsePreparedStackManagement1 } from "@alienplatform/platform-api/models";

let value: SyncListResponsePreparedStackManagement1 = {
  extend: {
    "key": [
      "<value>",
    ],
    "key1": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `extend`                                                                                                                          | Record<string, *models.SyncListResponsePreparedStackExtendUnion*[]>                                                               | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
