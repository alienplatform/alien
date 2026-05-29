# ResourceHeartbeatDataBuild

## Example Usage

```typescript
import { ResourceHeartbeatDataBuild } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataBuild = {
  data: {
    environmentVariableCount: 816046,
    managedEnvironmentId: "<id>",
    resourceGroupName: "<value>",
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
      lifecycle: "creating",
      partial: false,
      stale: true,
    },
    backend: "azureContainerApps",
  },
  resourceType: "build",
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `data`                      | *models.BuildHeartbeatData* | :heavy_check_mark:          | N/A                         |
| `resourceType`              | *"build"*                   | :heavy_check_mark:          | N/A                         |