# DataLocal10

## Example Usage

```typescript
import { DataLocal10 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal10 = {
  events: [],
  reachable: true,
  registryUrl: "https://fearless-exhaust.biz",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent49](../../models/operations/getrawresourceheartbeatevent49.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `reachable`                                                                                              | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `registryUrl`                                                                                            | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus49](../../models/operations/datastatus49.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"local"*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |