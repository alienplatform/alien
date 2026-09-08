# AiHeartbeatDataAwsBedrock

## Example Usage

```typescript
import { AiHeartbeatDataAwsBedrock } from "@alienplatform/manager-api/models";

let value: AiHeartbeatDataAwsBedrock = {
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
  region: "<value>",
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
  backend: "awsBedrock",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `availability`                                                             | [models.AiAvailabilityObservation](../models/aiavailabilityobservation.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `region`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.AiHeartbeatStatus](../models/aiheartbeatstatus.md)                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"awsBedrock"*                                                             | :heavy_check_mark:                                                         | N/A                                                                        |