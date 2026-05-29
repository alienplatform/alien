# ResourceHeartbeatDataBuild

## Example Usage

```typescript
import { ResourceHeartbeatDataBuild } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataBuild = {
  data: {
    environmentVariableCount: 816046,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    managedEnvironmentId: "<id>",
    resourceGroupName: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "stopped",
      partial: true,
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