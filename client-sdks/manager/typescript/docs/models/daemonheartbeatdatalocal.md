# DaemonHeartbeatDataLocal

## Example Usage

```typescript
import { DaemonHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: DaemonHeartbeatDataLocal = {
  commandSupported: true,
  events: [],
  imagePathPresent: true,
  runtimeId: "<id>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `commandSupported`                                                           | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `daemonInstance`                                                             | [models.LocalRuntimeUnitStatus](../models/localruntimeunitstatus.md)         | :heavy_minus_sign:                                                           | N/A                                                                          |
| `daemonName`                                                                 | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `events`                                                                     | [models.LocalRuntimeEventSnapshot](../models/localruntimeeventsnapshot.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `exitReason`                                                                 | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `imagePathPresent`                                                           | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `pid`                                                                        | *number*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `restartCount`                                                               | *number*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `runtimeId`                                                                  | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `status`                                                                     | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)       | :heavy_check_mark:                                                           | N/A                                                                          |
| `backend`                                                                    | *"local"*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |