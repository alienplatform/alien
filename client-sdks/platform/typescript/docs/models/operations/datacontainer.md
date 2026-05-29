# DataContainer

## Example Usage

```typescript
import { DataContainer } from "@alienplatform/platform-api/models/operations";

let value: DataContainer = {
  data: {
    attentionCount: 486054,
    containerId: "<id>",
    events: [],
    replicas: {},
    schedulingMode: "replicated",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "running",
      partial: true,
      stale: false,
    },
    backend: "horizonPlatform",
  },
  resourceType: "container",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `data`                  | *operations.DataUnion3* | :heavy_check_mark:      | N/A                     |
| `resourceType`          | *"container"*           | :heavy_check_mark:      | N/A                     |