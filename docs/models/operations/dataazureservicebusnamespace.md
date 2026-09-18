# DataAzureServiceBusNamespace

## Example Usage

```typescript
import { DataAzureServiceBusNamespace } from "@alienplatform/platform-api/models/operations";

let value: DataAzureServiceBusNamespace = {
  data: {
    name: "<value>",
    privateEndpointConnectionCount: 152029,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
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

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `data`                                               | [operations.Data5](../../models/operations/data5.md) | :heavy_check_mark:                                   | N/A                                                  |
| `resourceType`                                       | *"azure_service_bus_namespace"*                      | :heavy_check_mark:                                   | N/A                                                  |