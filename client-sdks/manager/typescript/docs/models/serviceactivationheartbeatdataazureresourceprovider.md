# ServiceActivationHeartbeatDataAzureResourceProvider

## Example Usage

```typescript
import { ServiceActivationHeartbeatDataAzureResourceProvider } from "@alienplatform/manager-api/models";

let value: ServiceActivationHeartbeatDataAzureResourceProvider = {
  events: [],
  namespace: "<value>",
  registered: true,
  resourceTypeCount: 306090,
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
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `events`                                                                                 | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                   | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `namespace`                                                                              | *string*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `providerId`                                                                             | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `registered`                                                                             | *boolean*                                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `registrationPolicy`                                                                     | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `registrationState`                                                                      | *string*                                                                                 | :heavy_minus_sign:                                                                       | N/A                                                                                      |
| `resourceTypeCount`                                                                      | *number*                                                                                 | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `status`                                                                                 | [models.ServiceActivationHeartbeatStatus](../models/serviceactivationheartbeatstatus.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `backend`                                                                                | *"azureResourceProvider"*                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |