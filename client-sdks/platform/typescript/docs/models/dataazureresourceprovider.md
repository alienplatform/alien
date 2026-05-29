# DataAzureResourceProvider

## Example Usage

```typescript
import { DataAzureResourceProvider } from "@alienplatform/platform-api/models";

let value: DataAzureResourceProvider = {
  events: [],
  namespace: "<value>",
  registered: false,
  resourceTypeCount: 249113,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: true,
  },
  backend: "azureResourceProvider",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `events`                                                                         | [models.SyncReconcileRequestEvent55](../models/syncreconcilerequestevent55.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `providerId`                                                                     | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `registered`                                                                     | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `registrationPolicy`                                                             | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `registrationState`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceTypeCount`                                                              | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus55](../models/heartbeatstatus55.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"azureResourceProvider"*                                                        | :heavy_check_mark:                                                               | N/A                                                                              |