# DataAzureServiceBusNamespace

## Example Usage

```typescript
import { DataAzureServiceBusNamespace } from "@alienplatform/platform-api/models";

let value: DataAzureServiceBusNamespace = {
  data: {
    events: [],
    name: "<value>",
    privateEndpointConnectionCount: 700227,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: true,
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