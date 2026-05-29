# DataNetwork

## Example Usage

```typescript
import { DataNetwork } from "@alienplatform/platform-api/models/operations";

let value: DataNetwork = {
  data: {
    events: [],
    isByoVnet: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "timed-out",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "scaling",
      partial: true,
      stale: false,
    },
    backend: "azureVnet",
  },
  resourceType: "network",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `data`                   | *operations.DataUnion10* | :heavy_check_mark:       | N/A                      |
| `resourceType`           | *"network"*              | :heavy_check_mark:       | N/A                      |