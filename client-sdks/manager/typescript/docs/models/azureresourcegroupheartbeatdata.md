# AzureResourceGroupHeartbeatData

## Example Usage

```typescript
import { AzureResourceGroupHeartbeatData } from "@alienplatform/manager-api/models";

let value: AzureResourceGroupHeartbeatData = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  managedTags: {},
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleted",
    partial: true,
    stale: false,
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `events`                                                                                   | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                     | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `location`                                                                                 | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `managedTags`                                                                              | Record<string, *string*>                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `name`                                                                                     | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `provisioningState`                                                                        | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `resourceId`                                                                               | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `status`                                                                                   | [models.AzureResourceGroupHeartbeatStatus](../models/azureresourcegroupheartbeatstatus.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |