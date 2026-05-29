# DataLocal7

## Example Usage

```typescript
import { DataLocal7 } from "@alienplatform/platform-api/models";

let value: DataLocal7 = {
  cloudMetadataSupported: false,
  events: [],
  name: "<value>",
  path: "/usr/src",
  pathExists: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "running",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cloudMetadataSupported`                                                         | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent30](../models/syncreconcilerequestevent30.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `isDirectory`                                                                    | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `path`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `pathExists`                                                                     | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus30](../models/heartbeatstatus30.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"local"*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |