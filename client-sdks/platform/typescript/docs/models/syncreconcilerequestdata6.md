# SyncReconcileRequestData6

## Example Usage

```typescript
import { SyncReconcileRequestData6 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData6 = {
  enabled: true,
  keyArn: "<value>",
  keySpec: "<value>",
  keyState: "<value>",
  keyUsage: "<value>",
  status: {
    health: "unhealthy",
    lifecycle: "stopped",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `enabled`                                                                  | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `keyArn`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `keySpec`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `keyState`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `keyUsage`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus70](../models/resourceheartbeatstatus70.md) | :heavy_check_mark:                                                         | N/A                                                                        |