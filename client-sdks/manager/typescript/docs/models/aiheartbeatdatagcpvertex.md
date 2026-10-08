# AiHeartbeatDataGcpVertex

## Example Usage

```typescript
import { AiHeartbeatDataGcpVertex } from "@alienplatform/manager-api/models";

let value: AiHeartbeatDataGcpVertex = {
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
  location: "<value>",
  project: "<value>",
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
  backend: "gcpVertex",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `availability`                                                             | [models.AiAvailabilityObservation](../models/aiavailabilityobservation.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `location`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `project`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.AiHeartbeatStatus](../models/aiheartbeatstatus.md)                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"gcpVertex"*                                                              | :heavy_check_mark:                                                         | N/A                                                                        |