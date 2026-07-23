# RemoteAzureContainerSas

Explicit fields of an Azure user-delegation SAS. Keeping the fields typed
lets clients independently validate container scope, permissions, protocol,
and expiry before constructing query parameters.

## Example Usage

```typescript
import { RemoteAzureContainerSas } from "@alienplatform/manager-api/models";

let value: RemoteAzureContainerSas = {
  accountName: "<value>",
  containerName: "<value>",
  expiresAt: "1750776331862",
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
};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `accountName`                                           | *string*                                                | :heavy_check_mark:                                      | Storage account named by the signed canonical resource. |
| `containerName`                                         | *string*                                                | :heavy_check_mark:                                      | Blob container named by the signed canonical resource.  |
| `expiresAt`                                             | *string*                                                | :heavy_check_mark:                                      | SAS validity end (`se`).                                |
| `permissions`                                           | *string*                                                | :heavy_check_mark:                                      | Canonically ordered SAS permissions (`sp`).             |
| `protocol`                                              | *string*                                                | :heavy_check_mark:                                      | Required transport protocol (`spr`).                    |
| `serviceVersion`                                        | *string*                                                | :heavy_check_mark:                                      | Storage authorization version (`sv`).                   |
| `signature`                                             | *string*                                                | :heavy_check_mark:                                      | HMAC-SHA256 signature (`sig`).                          |
| `signedKeyExpiry`                                       | *string*                                                | :heavy_check_mark:                                      | Delegation-key validity end (`ske`).                    |
| `signedKeyService`                                      | *string*                                                | :heavy_check_mark:                                      | Delegation-key service (`sks`).                         |
| `signedKeyStart`                                        | *string*                                                | :heavy_check_mark:                                      | Delegation-key validity start (`skt`).                  |
| `signedKeyVersion`                                      | *string*                                                | :heavy_check_mark:                                      | Delegation-key version (`skv`).                         |
| `signedObjectId`                                        | *string*                                                | :heavy_check_mark:                                      | Object ID that requested the delegation key (`skoid`).  |
| `signedResource`                                        | *string*                                                | :heavy_check_mark:                                      | Signed resource kind (`sr`).                            |
| `signedTenantId`                                        | *string*                                                | :heavy_check_mark:                                      | Tenant ID that issued the delegation key (`sktid`).     |
| `startsAt`                                              | *string*                                                | :heavy_check_mark:                                      | SAS validity start (`st`).                              |