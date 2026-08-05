# ResourceHeartbeatDataAzureServiceBusNamespace

## Example Usage

```typescript
import { ResourceHeartbeatDataAzureServiceBusNamespace } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAzureServiceBusNamespace = {
  data: {
    name: "<value>",
    privateEndpointConnectionCount: 289000,
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
  },
  resourceType: "azure_service_bus_namespace",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `data`                                                                                             | [models.AzureServiceBusNamespaceHeartbeatData](../models/azureservicebusnamespaceheartbeatdata.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `resourceType`                                                                                     | *"azure_service_bus_namespace"*                                                                    | :heavy_check_mark:                                                                                 | N/A                                                                                                |