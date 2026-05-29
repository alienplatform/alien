# DataAzureResourceGroup

## Example Usage

```typescript
import { DataAzureResourceGroup } from "@alienplatform/platform-api/models/operations";

let value: DataAzureResourceGroup = {
  data: {
    events: [],
    managedTags: {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "creating",
      partial: true,
      stale: true,
    },
  },
  resourceType: "azure_resource_group",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `data`                                               | [operations.Data2](../../models/operations/data2.md) | :heavy_check_mark:                                   | N/A                                                  |
| `resourceType`                                       | *"azure_resource_group"*                             | :heavy_check_mark:                                   | N/A                                                  |