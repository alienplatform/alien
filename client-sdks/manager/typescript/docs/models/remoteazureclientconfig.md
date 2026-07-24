# RemoteAzureClientConfig

Response-safe Azure client configuration. It contains one container-bound
user-delegation SAS and no OAuth or refreshable identity source.

## Example Usage

```typescript
import { RemoteAzureClientConfig } from "@alienplatform/manager-api/models";

let value: RemoteAzureClientConfig = {
  credentials: {
    sas: {
      accountName: "<value>",
      containerName: "<value>",
      expiresAt: "1762181110811",
      permissions: "<value>",
      protocol: "<value>",
      serviceVersion: "<value>",
      signature: "<value>",
      signedKeyExpiry: "<value>",
      signedKeyService: "<value>",
      signedKeyStart: "<value>",
      signedKeyVersion: "<value>",
      signedObjectId: "<id>",
      signedResource: "<value>",
      signedTenantId: "<id>",
      startsAt: "<value>",
    },
    type: "containerSas",
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