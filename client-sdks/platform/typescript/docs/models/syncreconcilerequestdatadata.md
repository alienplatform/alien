# SyncReconcileRequestDataData

## Example Usage

```typescript
import { SyncReconcileRequestDataData } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestDataData = {
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `details`                                                                                              | Record<string, *any*>                                                                                  | :heavy_minus_sign:                                                                                     | Type-specific fields the extension's controller reports, which Alien does not read.                    |
| `status`                                                                                               | [models.ResourceHeartbeatStatus82](../models/resourceheartbeatstatus82.md)                             | :heavy_check_mark:                                                                                     | The status fields every heartbeat carries, so a custom heartbeat's health reads like a<br/>built-in one's. |