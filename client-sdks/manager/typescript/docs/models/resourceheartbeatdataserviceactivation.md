# ResourceHeartbeatDataServiceActivation

## Example Usage

```typescript
import { ResourceHeartbeatDataServiceActivation } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataServiceActivation = {
  data: {
    events: [],
    namespace: "<value>",
    registered: true,
    resourceTypeCount: 440272,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopping",
      partial: true,
      stale: false,
    },
    backend: "azureResourceProvider",
  },
  resourceType: "service_activation",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `data`                                  | *models.ServiceActivationHeartbeatData* | :heavy_check_mark:                      | N/A                                     |
| `resourceType`                          | *"service_activation"*                  | :heavy_check_mark:                      | N/A                                     |