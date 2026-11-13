# RedisHeartbeatStatus

Same fields as every other resource heartbeat status.

## Example Usage

```typescript
import { RedisHeartbeatStatus } from "@alienplatform/manager-api/models";

let value: RedisHeartbeatStatus = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "running",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `collectionIssues`                                                         | [models.HeartbeatCollectionIssue](../models/heartbeatcollectionissue.md)[] | :heavy_check_mark:                                                         | N/A                                                                        |
| `health`                                                                   | [models.ObservedHealth](../models/observedhealth.md)                       | :heavy_check_mark:                                                         | N/A                                                                        |
| `lifecycle`                                                                | [models.ProviderLifecycleState](../models/providerlifecyclestate.md)       | :heavy_check_mark:                                                         | N/A                                                                        |
| `message`                                                                  | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `partial`                                                                  | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `stale`                                                                    | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
