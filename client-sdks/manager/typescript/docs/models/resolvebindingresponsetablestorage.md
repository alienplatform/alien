# ResolveBindingResponseTablestorage

Azure Table Storage KV table and a storage-audience access token.

## Example Usage

```typescript
import { ResolveBindingResponseTablestorage } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseTablestorage = {
  binding: {
    accountName: "<value>",
    resourceGroupName: "<value>",
    tableName: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    subscriptionId: "<id>",
    tenantId: "<id>",
  },
  expiresAt: "1743538811558",
  service: "tablestorage",
};
```

## Fields

| Field                                                                                                                           | Type                                                                                                                            | Required                                                                                                                        | Description                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                       | [models.RemoteTableStorageKvBinding](../models/remotetablestoragekvbinding.md)                                                  | :heavy_check_mark:                                                                                                              | Concrete Azure Table Storage KV topology returned to remote clients.                                                            |
| `clientConfig`                                                                                                                  | [models.RemoteAzureClientConfig](../models/remoteazureclientconfig.md)                                                          | :heavy_check_mark:                                                                                                              | Response-safe Azure client configuration containing one storage-audience<br/>access token for the stack's Remote Bindings identity. |
| `expiresAt`                                                                                                                     | *string*                                                                                                                        | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `service`                                                                                                                       | *"tablestorage"*                                                                                                                | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |