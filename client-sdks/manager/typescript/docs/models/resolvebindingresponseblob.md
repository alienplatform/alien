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
      token: "<value>",
      type: "accessToken",
    },
    subscriptionId: "<id>",
    tenantId: "<id>",
  },
  expiresAt: "1762181110811",
  service: "blob",
};
```

## Fields

| Field                                                                                                                           | Type                                                                                                                            | Required                                                                                                                        | Description                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                       | [models.RemoteBlobStorageBinding](../models/remoteblobstoragebinding.md)                                                        | :heavy_check_mark:                                                                                                              | Concrete Azure Blob Storage topology returned to remote clients.                                                                |
| `clientConfig`                                                                                                                  | [models.RemoteAzureClientConfig](../models/remoteazureclientconfig.md)                                                          | :heavy_check_mark:                                                                                                              | Response-safe Azure client configuration containing one storage-audience<br/>access token for the stack's Remote Bindings identity. |
| `expiresAt`                                                                                                                     | *string*                                                                                                                        | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `service`                                                                                                                       | *"blob"*                                                                                                                        | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
