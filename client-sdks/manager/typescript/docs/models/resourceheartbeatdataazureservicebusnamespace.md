# ResourceHeartbeatDataAzureServiceBusNamespace

## Example Usage

```typescript
import { ResourceHeartbeatDataAzureServiceBusNamespace } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAzureServiceBusNamespace = {
  data: {
    events: [],
    name: "<value>",
    privateEndpointConnectionCount: 297212,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "updating",
      partial: false,
      stale: false,
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