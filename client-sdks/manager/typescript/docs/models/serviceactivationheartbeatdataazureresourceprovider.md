# ServiceActivationHeartbeatDataAzureResourceProvider

## Example Usage

```typescript
import { ServiceActivationHeartbeatDataAzureResourceProvider } from "@alienplatform/manager-api/models";

let value: ServiceActivationHeartbeatDataAzureResourceProvider = {
  namespace: "<value>",
  registered: true,
  resourceTypeCount: 15021,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "azureResourceProvider",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `namespace`                                                                              | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `providerId`                                                                             | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `registered`                                                                             | *boolean*                                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `registrationPolicy`                                                                     | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `registrationState`                                                                      | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `resourceTypeCount`                                                                      | *number*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `status`                                                                                 | [models.ServiceActivationHeartbeatStatus](../models/serviceactivationheartbeatstatus.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `backend`                                                                                | *"azureResourceProvider"*                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |