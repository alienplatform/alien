# ResourceHeartbeatDataAi

## Example Usage

```typescript
import { ResourceHeartbeatDataAi } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAi = {
  data: {
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
  },
  resourceType: "ai",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `data`                   | *models.AiHeartbeatData* | :heavy_check_mark:       | N/A                      |
| `resourceType`           | *"ai"*                   | :heavy_check_mark:       | N/A                      |