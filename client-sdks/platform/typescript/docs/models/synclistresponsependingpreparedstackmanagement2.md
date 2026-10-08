# SyncListResponsePendingPreparedStackManagement2

## Example Usage

```typescript
import { SyncListResponsePendingPreparedStackManagement2 } from "@alienplatform/platform-api/models";

let value: SyncListResponsePendingPreparedStackManagement2 = {
  override: {
    "key": [],
    "key1": [
      "<value>",
    ],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncListResponsePendingPreparedStackOverrideUnion*[]>                                                      | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |
