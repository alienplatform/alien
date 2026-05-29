# DataLocal8

## Example Usage

```typescript
import { DataLocal8 } from "@alienplatform/platform-api/models";

let value: DataLocal8 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-07-08T01:45:59.103Z"),
      severity: "error",
    },
  ],
  path: "/usr/local/src",
  pathExists: true,
  secretMetadataListed: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `events`                                                                                      | [models.SyncReconcileRequestEvent35](../models/syncreconcilerequestevent35.md)[]              | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `isDirectory`                                                                                 | *boolean*                                                                                     | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `modifiedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `path`                                                                                        | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `pathExists`                                                                                  | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `readonly`                                                                                    | *boolean*                                                                                     | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `secretMetadataListed`                                                                        | *boolean*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `status`                                                                                      | [models.HeartbeatStatus35](../models/heartbeatstatus35.md)                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `backend`                                                                                     | *"local"*                                                                                     | :heavy_check_mark:                                                                            | N/A                                                                                           |