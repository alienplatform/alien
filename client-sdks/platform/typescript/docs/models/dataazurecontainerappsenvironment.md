# DataAzureContainerAppsEnvironment

## Example Usage

```typescript
import { DataAzureContainerAppsEnvironment } from "@alienplatform/platform-api/models";

let value: DataAzureContainerAppsEnvironment = {
  data: {
    events: [],
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "updating",
      partial: true,
      stale: false,
    },
    workloadProfileCount: 388415,
    workloadProfiles: [],
  },
  resourceType: "azure_container_apps_environment",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `data`                                                                     | [models.SyncReconcileRequestData4](../models/syncreconcilerequestdata4.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `resourceType`                                                             | *"azure_container_apps_environment"*                                       | :heavy_check_mark:                                                         | N/A                                                                        |