# DataAi

## Example Usage

```typescript
import { DataAi } from "@alienplatform/platform-api/models/operations";

let value: DataAi = {
  data: {
    availability: {
      catalogRevision: "<value>",
      models: [
        {
          accessTest: "not-checked",
          availability: "unknown",
          blockers: [],
          clientApis: [],
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

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `data`                   | *operations.DataUnion16* | :heavy_check_mark:       | N/A                      |
| `resourceType`           | *"ai"*                   | :heavy_check_mark:       | N/A                      |