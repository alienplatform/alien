# DataAi

## Example Usage

```typescript
import { DataAi } from "@alienplatform/platform-api/models";

let value: DataAi = {
  data: {
    availability: {
      catalogRevision: "<value>",
      models: [
        {
          accessTest: "failed",
          availability: "blocked",
          blockers: [],
          clientApis: [
            "open-ai-chat-completions",
          ],
          publicModelId: "<id>",
        },
      ],
      source: "anthropic",
    },
    location: "<value>",
    project: "<value>",
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "unknown",
      partial: false,
      stale: false,
    },
    backend: "gcpVertex",
  },
  resourceType: "ai",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `data`                                   | *models.SyncReconcileRequestDataUnion16* | :heavy_check_mark:                       | N/A                                      |
| `resourceType`                           | *"ai"*                                   | :heavy_check_mark:                       | N/A                                      |