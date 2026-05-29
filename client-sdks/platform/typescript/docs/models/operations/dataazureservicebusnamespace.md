# DataAzureServiceBusNamespace

## Example Usage

```typescript
import { DataAzureServiceBusNamespace } from "@alienplatform/platform-api/models/operations";

let value: DataAzureServiceBusNamespace = {
  data: {
    events: [],
    name: "<value>",
    privateEndpointConnectionCount: 700227,
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "unknown",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_service_bus_namespace",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `data`                                               | [operations.Data5](../../models/operations/data5.md) | :heavy_check_mark:                                   | N/A                                                  |
| `resourceType`                                       | *"azure_service_bus_namespace"*                      | :heavy_check_mark:                                   | N/A                                                  |