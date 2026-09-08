# AiHeartbeatDataAzureFoundry

## Example Usage

```typescript
import { AiHeartbeatDataAzureFoundry } from "@alienplatform/manager-api/models";

let value: AiHeartbeatDataAzureFoundry = {
  accountName: "<value>",
  availability: {
    catalogRevision: "<value>",
    models: [
      {
        accessTest: "not-checked",
        availability: "available",
        blockers: [
          "agreement-required",
        ],
        clientApis: [
          "anthropic-messages",
        ],
        publicModelId: "<id>",
      },
    ],
    source: "aws-bedrock",
  },
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "azureFoundry",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `accountName`                                                              | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `availability`                                                             | [models.AiAvailabilityObservation](../models/aiavailabilityobservation.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `endpoint`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `location`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceGroup`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.AiHeartbeatStatus](../models/aiheartbeatstatus.md)                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"azureFoundry"*                                                           | :heavy_check_mark:                                                         | N/A                                                                        |