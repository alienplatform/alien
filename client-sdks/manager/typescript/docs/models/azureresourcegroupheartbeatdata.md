# AzureResourceGroupHeartbeatData

## Example Usage

```typescript
import { AzureResourceGroupHeartbeatData } from "@alienplatform/manager-api/models";

let value: AzureResourceGroupHeartbeatData = {
  managedTags: {
    "key": "<value>",
    "key1": "<value>",
  },
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: false,
    stale: true,
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `location`                                                                                 | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `managedTags`                                                                              | Record<string, *string*>                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `name`                                                                                     | *string*                                                                                   | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `provisioningState`                                                                        | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `resourceId`                                                                               | *string*                                                                                   | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `status`                                                                                   | [models.AzureResourceGroupHeartbeatStatus](../models/azureresourcegroupheartbeatstatus.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |