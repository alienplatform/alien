# ResourceHeartbeatDataAzureContainerAppsEnvironment

## Example Usage

```typescript
import { ResourceHeartbeatDataAzureContainerAppsEnvironment } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAzureContainerAppsEnvironment = {
  data: {
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
      lifecycle: "creating",
      partial: true,
      stale: false,
    },
    workloadProfileCount: 511598,
    workloadProfiles: [
      {
        name: "<value>",
        workloadProfileType: "<value>",
      },
    ],
  },
  resourceType: "azure_container_apps_environment",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `data`                                                                                                       | [models.AzureContainerAppsEnvironmentHeartbeatData](../models/azurecontainerappsenvironmentheartbeatdata.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `resourceType`                                                                                               | *"azure_container_apps_environment"*                                                                         | :heavy_check_mark:                                                                                           | N/A                                                                                                          |