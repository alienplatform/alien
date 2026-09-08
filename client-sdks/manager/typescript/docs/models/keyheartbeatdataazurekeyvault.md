# KeyHeartbeatDataAzureKeyVault

## Example Usage

```typescript
import { KeyHeartbeatDataAzureKeyVault } from "@alienplatform/manager-api/models";

let value: KeyHeartbeatDataAzureKeyVault = {
  data: {
    keyId: "<id>",
    keyOperations: [
      "<value 1>",
    ],
    keyType: "<value>",
    status: {
      health: "healthy",
      lifecycle: "running",
    },
  },
  provider: "azure-key-vault",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `data`                                                                             | [models.AzureKeyVaultKeyHeartbeatData](../models/azurekeyvaultkeyheartbeatdata.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `provider`                                                                         | *"azure-key-vault"*                                                                | :heavy_check_mark:                                                                 | N/A                                                                                |