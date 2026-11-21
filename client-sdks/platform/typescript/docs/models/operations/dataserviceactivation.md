# DataServiceActivation

## Example Usage

```typescript
import { DataServiceActivation } from "@alienplatform/platform-api/models/operations";

let value: DataServiceActivation = {
  data: {
    enabled: true,
    projectId: "<id>",
    serviceName: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "collection-failed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "deleted",
      partial: false,
      stale: false,
    },
    backend: "gcpServiceUsage",
  },
  resourceType: "service_activation",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `data`                                                                                               | *operations.DataUnion16*                                                                             | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `resourceType`                                                                                       | [operations.ResourceTypeServiceActivation](../../models/operations/resourcetypeserviceactivation.md) | :heavy_check_mark:                                                                                   | N/A                                                                                                  |