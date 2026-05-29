# DataAzureResourceProvider

## Example Usage

```typescript
import { DataAzureResourceProvider } from "@alienplatform/platform-api/models/operations";

let value: DataAzureResourceProvider = {
  events: [],
  namespace: "<value>",
  registered: false,
  resourceTypeCount: 249113,
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
    stale: false,
  },
  backend: "azureResourceProvider",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent55](../../models/operations/getrawresourceheartbeatevent55.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `namespace`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `providerId`                                                                                             | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `registered`                                                                                             | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `registrationPolicy`                                                                                     | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `registrationState`                                                                                      | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `resourceTypeCount`                                                                                      | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus55](../../models/operations/datastatus55.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"azureResourceProvider"*                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |