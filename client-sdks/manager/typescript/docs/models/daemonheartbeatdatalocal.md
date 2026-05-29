# DaemonHeartbeatDataLocal

## Example Usage

```typescript
import { DaemonHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: DaemonHeartbeatDataLocal = {
  commandSupported: true,
  daemonName: "<value>",
  events: [],
  imagePathPresent: true,
  runtimeId: "<id>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `commandSupported`                                                     | *boolean*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |
| `daemonName`                                                           | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `events`                                                               | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                 | :heavy_check_mark:                                                     | N/A                                                                    |
| `exitReason`                                                           | *string*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `imagePathPresent`                                                     | *boolean*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |
| `pid`                                                                  | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `restartCount`                                                         | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `runtimeId`                                                            | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `status`                                                               | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `backend`                                                              | *"local"*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |