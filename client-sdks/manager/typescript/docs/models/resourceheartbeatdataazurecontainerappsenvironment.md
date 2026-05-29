# ResourceHeartbeatDataAzureContainerAppsEnvironment

## Example Usage

```typescript
import { ResourceHeartbeatDataAzureContainerAppsEnvironment } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAzureContainerAppsEnvironment = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "stopping",
      partial: false,
      stale: false,
    },
    workloadProfileCount: 762670,
    workloadProfiles: [],
  },
  resourceType: "azure_container_apps_environment",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `data`                                                                                                       | [models.AzureContainerAppsEnvironmentHeartbeatData](../models/azurecontainerappsenvironmentheartbeatdata.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `resourceType`                                                                                               | *"azure_container_apps_environment"*                                                                         | :heavy_check_mark:                                                                                           | N/A                                                                                                          |