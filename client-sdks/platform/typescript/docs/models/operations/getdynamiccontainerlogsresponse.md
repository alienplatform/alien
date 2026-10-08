# GetDynamicContainerLogsResponse

Container logs.

## Example Usage

```typescript
import { GetDynamicContainerLogsResponse } from "@alienplatform/platform-api/models/operations";

let value: GetDynamicContainerLogsResponse = {
  logs: [
    {
      timestamp: new Date("2025-09-26T06:16:14.674Z"),
      level: "<value>",
      message: "<value>",
      replicaId: "<id>",
    },
  ],
  numHits: 8751.01,
  nextCursor: "<value>",
  partial: false,
  errorCount: 1864.94,
};
```

## Fields

| Field                                              | Type                                               | Required                                           | Description                                        |
| -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------- |
| `logs`                                             | [operations.Log](../../models/operations/log.md)[] | :heavy_check_mark:                                 | N/A                                                |
| `numHits`                                          | *number*                                           | :heavy_check_mark:                                 | N/A                                                |
| `nextCursor`                                       | *string*                                           | :heavy_check_mark:                                 | N/A                                                |
| `partial`                                          | *boolean*                                          | :heavy_check_mark:                                 | N/A                                                |
| `errorCount`                                       | *number*                                           | :heavy_check_mark:                                 | N/A                                                |