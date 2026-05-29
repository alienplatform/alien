# DataLocal10

## Example Usage

```typescript
import { DataLocal10 } from "@alienplatform/platform-api/models";

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

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `events`                                                                         | [models.SyncReconcileRequestEvent49](../models/syncreconcilerequestevent49.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `reachable`                                                                      | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `registryUrl`                                                                    | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus49](../models/heartbeatstatus49.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"local"*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |