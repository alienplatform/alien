# ResolveBindingResponseBlob

Azure Blob Storage and an exact container-scoped SAS.

## Example Usage

```typescript
import { ResolveBindingResponseBlob } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseBlob = {
  binding: {
    accountName: "<value>",
    containerName: "<value>",
  },
  clientConfig: {
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
  },
  expiresAt: "1759301232953",
  service: "blob",
};
```

## Fields

| Field                                                                                                                                      | Type                                                                                                                                       | Required                                                                                                                                   | Description                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `binding`                                                                                                                                  | [models.RemoteBlobStorageBinding](../models/remoteblobstoragebinding.md)                                                                   | :heavy_check_mark:                                                                                                                         | Concrete Azure Blob Storage topology returned to remote clients.                                                                           |
| `clientConfig`                                                                                                                             | [models.RemoteAzureClientConfig](../models/remoteazureclientconfig.md)                                                                     | :heavy_check_mark:                                                                                                                         | Response-safe Azure client configuration. It contains one container-bound<br/>user-delegation SAS and no OAuth or refreshable identity source. |
| `expiresAt`                                                                                                                                | *string*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | N/A                                                                                                                                        |
| `service`                                                                                                                                  | *"blob"*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | N/A                                                                                                                                        |