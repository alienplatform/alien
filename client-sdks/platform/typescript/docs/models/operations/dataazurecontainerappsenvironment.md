# DataAzureContainerAppsEnvironment

## Example Usage

```typescript
import { DataAzureContainerAppsEnvironment } from "@alienplatform/platform-api/models/operations";

let value: DataAzureContainerAppsEnvironment = {
  data: {
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "deleted",
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

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `data`                                                                                                                       | [operations.GetResourceDeploymentDetailData4](../../models/operations/getresourcedeploymentdetaildata4.md)                   | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |
| `resourceType`                                                                                                               | [operations.ResourceTypeAzureContainerAppsEnvironment](../../models/operations/resourcetypeazurecontainerappsenvironment.md) | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |