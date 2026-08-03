# RemoteAzureClientConfig

Response-safe Azure client configuration containing one storage-audience
access token for the stack's Remote Bindings identity.

## Example Usage

```typescript
import { RemoteAzureClientConfig } from "@alienplatform/manager-api/models";

let value: RemoteAzureClientConfig = {
  credentials: {
    token: "<value>",
    type: "accessToken",
  },
  subscriptionId: "<id>",
  tenantId: "<id>",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `credentials`                                                        | *models.RemoteAzureCredentials*                                      | :heavy_check_mark:                                                   | The only Azure credential form remote binding resolution can return. |
| `region`                                                             | *string*                                                             | :heavy_minus_sign:                                                   | Azure region configured for the deployment.                          |
| `subscriptionId`                                                     | *string*                                                             | :heavy_check_mark:                                                   | Azure subscription containing the storage account.                   |
| `tenantId`                                                           | *string*                                                             | :heavy_check_mark:                                                   | Azure tenant owning the identity.                                    |