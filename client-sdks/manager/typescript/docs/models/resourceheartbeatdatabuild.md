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
          reason: "not-installed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "updating",
      partial: false,
      stale: false,
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