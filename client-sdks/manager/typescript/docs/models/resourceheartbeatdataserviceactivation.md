# ResourceHeartbeatDataServiceActivation

## Example Usage

```typescript
import { ResourceHeartbeatDataServiceActivation } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataServiceActivation = {
  data: {
    namespace: "<value>",
    registered: true,
    resourceTypeCount: 100188,
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "unknown",
      partial: false,
      stale: true,
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