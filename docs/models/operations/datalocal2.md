# DataLocal2

## Example Usage

```typescript
import { DataLocal2 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal2 = {
  commandSupported: true,
  events: [],
  imagePathPresent: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  triggerCount: 963366,
  backend: "local",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `commandSupported`                                               | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `cpu`                                                            | *operations.CpuUnion2*                                           | :heavy_minus_sign:                                               | N/A                                                              |
| `events`                                                         | [operations.Event2](../../models/operations/event2.md)[]         | :heavy_check_mark:                                               | N/A                                                              |
| `imagePathPresent`                                               | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `memory`                                                         | *operations.MemoryUnion2*                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `pid`                                                            | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `process`                                                        | *operations.ProcessUnion*                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `readinessProbeOk`                                               | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `status`                                                         | [operations.DataStatus9](../../models/operations/datastatus9.md) | :heavy_check_mark:                                               | N/A                                                              |
| `triggerCount`                                                   | *number*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | *"local"*                                                        | :heavy_check_mark:                                               | N/A                                                              |