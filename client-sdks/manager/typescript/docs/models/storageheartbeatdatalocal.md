# StorageHeartbeatDataLocal

## Example Usage

```typescript
import { StorageHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: StorageHeartbeatDataLocal = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  path: "/private/tmp",
  pathExists: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `events`                                                                                      | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                        | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `isDirectory`                                                                                 | *boolean*                                                                                     | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `modifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `path`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `pathExists`                                                                                  | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `readonly`                                                                                    | *boolean*                                                                                     | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [models.StorageHeartbeatStatus](../models/storageheartbeatstatus.md)                          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *"local"*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |