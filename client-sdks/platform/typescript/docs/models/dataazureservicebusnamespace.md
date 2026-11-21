# DataAzureServiceBusNamespace

## Example Usage

```typescript
import { DataAzureServiceBusNamespace } from "@alienplatform/platform-api/models";

let value: DataAzureServiceBusNamespace = {
  data: {
    name: "<value>",
    privateEndpointConnectionCount: 152029,
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "creating",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_service_bus_namespace",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `data`                                                                     | [models.SyncReconcileRequestData5](../models/syncreconcilerequestdata5.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `resourceType`                                                             | *"azure_service_bus_namespace"*                                            | :heavy_check_mark:                                                         | N/A                                                                        |