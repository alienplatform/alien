# AzureKeyVaultKeyHeartbeatData

## Example Usage

```typescript
import { AzureKeyVaultKeyHeartbeatData } from "@alienplatform/manager-api/models";

let value: AzureKeyVaultKeyHeartbeatData = {
  keyId: "<id>",
  keyOperations: [],
  keyType: "<value>",
  status: {
    health: "healthy",
    lifecycle: "running",
  },
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `enabled`                                                    | *boolean*                                                    | :heavy_minus_sign:                                           | N/A                                                          |
| `keyId`                                                      | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `keyOperations`                                              | *string*[]                                                   | :heavy_check_mark:                                           | N/A                                                          |
| `keyType`                                                    | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `recoveryLevel`                                              | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `status`                                                     | [models.KeyHeartbeatStatus](../models/keyheartbeatstatus.md) | :heavy_check_mark:                                           | N/A                                                          |