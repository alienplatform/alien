# QueueHeartbeatStatus

## Example Usage

```typescript
import { QueueHeartbeatStatus } from "@alienplatform/manager-api/models";

let value: QueueHeartbeatStatus = {
  collectionIssues: [],
  health: "unhealthy",
  lifecycle: "scaling",
  partial: true,
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