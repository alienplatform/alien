# RemoteAzureCredentialsContainerSas

User-delegation SAS signed for exactly one container.

## Example Usage

```typescript
import { RemoteAzureCredentialsContainerSas } from "@alienplatform/manager-api/models";

let value: RemoteAzureCredentialsContainerSas = {
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
};
```

## Fields

| Field                                                                                                                                                                                                  | Type                                                                                                                                                                                                   | Required                                                                                                                                                                                               | Description                                                                                                                                                                                            |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `sas`                                                                                                                                                                                                  | [models.RemoteAzureContainerSas](../models/remoteazurecontainersas.md)                                                                                                                                 | :heavy_check_mark:                                                                                                                                                                                     | Explicit fields of an Azure user-delegation SAS. Keeping the fields typed<br/>lets clients independently validate container scope, permissions, protocol,<br/>and expiry before constructing query parameters. |
| `type`                                                                                                                                                                                                 | [models.RemoteAzureCredentialsType](../models/remoteazurecredentialstype.md)                                                                                                                           | :heavy_check_mark:                                                                                                                                                                                     | N/A                                                                                                                                                                                                    |