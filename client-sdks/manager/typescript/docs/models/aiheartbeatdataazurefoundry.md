# AiHeartbeatDataAzureFoundry

## Example Usage

```typescript
import { AiHeartbeatDataAzureFoundry } from "@alienplatform/manager-api/models";

let value: AiHeartbeatDataAzureFoundry = {
  accountName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "azureFoundry",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `accountName`                                              | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `endpoint`                                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `location`                                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `resourceGroup`                                            | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.AiHeartbeatStatus](../models/aiheartbeatstatus.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"azureFoundry"*                                           | :heavy_check_mark:                                         | N/A                                                        |
