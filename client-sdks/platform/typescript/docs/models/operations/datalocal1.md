# DataLocal1

## Example Usage

```typescript
import { DataLocal1 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal1 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-07-12T08:12:10.995Z"),
      severity: "error",
    },
  ],
  path: "/usr/share",
  pathExists: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `events`                                                                                               | [operations.GetRawResourceHeartbeatEvent4](../../models/operations/getrawresourceheartbeatevent4.md)[] | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `isDirectory`                                                                                          | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `modifiedAt`                                                                                           | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)          | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `path`                                                                                                 | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `pathExists`                                                                                           | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `readonly`                                                                                             | *boolean*                                                                                              | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `status`                                                                                               | [operations.DataStatus4](../../models/operations/datastatus4.md)                                       | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `backend`                                                                                              | *"local"*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |