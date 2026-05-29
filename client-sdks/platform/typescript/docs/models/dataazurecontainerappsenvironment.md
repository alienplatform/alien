# DataAzureContainerAppsEnvironment

## Example Usage

```typescript
import { DataAzureContainerAppsEnvironment } from "@alienplatform/platform-api/models";

let value: DataAzureContainerAppsEnvironment = {
  data: {
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "scaling",
      partial: true,
      stale: true,
    },
    workloadProfileCount: 415280,
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