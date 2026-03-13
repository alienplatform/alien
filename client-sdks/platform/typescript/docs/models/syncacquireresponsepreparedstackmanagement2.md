# SyncAcquireResponsePreparedStackManagement2

## Example Usage

```typescript
import { SyncAcquireResponsePreparedStackManagement2 } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponsePreparedStackManagement2 = {
  override: {
    "key": [
      {
        description: "pace quash ah gadzooks slake think pro politely poor",
        id: "<id>",
        platforms: {},
      },
    ],
    "key1": [],
    "key2": [],
  },
};
```

## Fields

| Field                                                                                                                             | Type                                                                                                                              | Required                                                                                                                          | Description                                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `override`                                                                                                                        | Record<string, *models.SyncAcquireResponsePreparedStackOverrideUnion*[]>                                                          | :heavy_check_mark:                                                                                                                | Permission profile that maps resources to permission sets<br/>Key can be "*" for all resources or resource name for specific resource |