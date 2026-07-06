# DataAzureResourceProvider

## Example Usage

```typescript
import { DataAzureResourceProvider } from "@alienplatform/platform-api/models";

let value: DataAzureResourceProvider = {
  namespace: "<value>",
  registered: true,
  resourceTypeCount: 563831,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "azureResourceProvider",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `namespace`                                                | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `providerId`                                               | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `registered`                                               | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `registrationPolicy`                                       | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `registrationState`                                        | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `resourceTypeCount`                                        | *number*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus59](../models/heartbeatstatus59.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"azureResourceProvider"*                                  | :heavy_check_mark:                                         | N/A                                                        |