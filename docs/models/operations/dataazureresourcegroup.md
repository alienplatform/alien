# DataAzureResourceGroup

## Example Usage

```typescript
import { DataAzureResourceGroup } from "@alienplatform/platform-api/models/operations";

let value: DataAzureResourceGroup = {
  data: {
    managedTags: {},
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "running",
      partial: true,
      stale: false,
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