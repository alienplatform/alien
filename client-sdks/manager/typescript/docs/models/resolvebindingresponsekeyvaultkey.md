# ResolveBindingResponseKeyVaultKey

Azure Key Vault key and a vault-audience access token.

## Example Usage

```typescript
import { ResolveBindingResponseKeyVaultKey } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseKeyVaultKey = {
  binding: {
    keyId: "<id>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    subscriptionId: "<id>",
    tenantId: "<id>",
  },
  expiresAt: "1740403538042",
  service: "key-vault-key",
};
```

## Fields

| Field                                                                                                                           | Type                                                                                                                            | Required                                                                                                                        | Description                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                       | [models.RemoteAzureKeyVaultKeyBinding](../models/remoteazurekeyvaultkeybinding.md)                                              | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `clientConfig`                                                                                                                  | [models.RemoteAzureClientConfig](../models/remoteazureclientconfig.md)                                                          | :heavy_check_mark:                                                                                                              | Response-safe Azure client configuration containing one storage-audience<br/>access token for the stack's Remote Bindings identity. |
| `expiresAt`                                                                                                                     | *string*                                                                                                                        | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `service`                                                                                                                       | *"key-vault-key"*                                                                                                               | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |