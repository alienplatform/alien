# WorkloadHeartbeatStatus

## Example Usage

```typescript
import { WorkloadHeartbeatStatus } from "@alienplatform/manager-api/models";

let value: WorkloadHeartbeatStatus = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "running",
  partial: true,
  stale: false,
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