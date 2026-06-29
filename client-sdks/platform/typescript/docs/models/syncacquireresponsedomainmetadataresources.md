# SyncAcquireResponseDomainMetadataResources

Certificate and DNS metadata for a public resource.

The direct fields describe the primary endpoint hostname. `endpoints`
contains endpoint-scoped metadata keyed by endpoint name. `aliases` contains
additional managed hostnames that route directly to the primary endpoint.

## Example Usage

```typescript
import { SyncAcquireResponseDomainMetadataResources } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDomainMetadataResources = {
  certificateId: "<id>",
  certificateStatus: "renewing",
  dnsStatus: "active",
  fqdn: "<value>",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `aliases`                                                                                        | [models.SyncAcquireResponseAlias](../models/syncacquireresponsealias.md)[]                       | :heavy_minus_sign:                                                                               | Additional managed hostnames for the resource.                                                   |
| `certificateChain`                                                                               | *string*                                                                                         | :heavy_minus_sign:                                                                               | Full PEM certificate chain (only present if status is "issued").                                 |
| `certificateId`                                                                                  | *string*                                                                                         | :heavy_check_mark:                                                                               | Certificate ID (for tracking/logging).                                                           |
| `certificateStatus`                                                                              | [models.SyncAcquireResponseCertificateStatus](../models/syncacquireresponsecertificatestatus.md) | :heavy_check_mark:                                                                               | Certificate status in the certificate lifecycle                                                  |
| `dnsError`                                                                                       | *string*                                                                                         | :heavy_minus_sign:                                                                               | Last DNS error message.                                                                          |
| `dnsStatus`                                                                                      | [models.SyncAcquireResponseDnsStatus](../models/syncacquireresponsednsstatus.md)                 | :heavy_check_mark:                                                                               | DNS record status in the DNS lifecycle                                                           |
| `endpoints`                                                                                      | Record<string, [models.SyncAcquireResponseEndpoints](../models/syncacquireresponseendpoints.md)> | :heavy_minus_sign:                                                                               | Endpoint-scoped metadata keyed by endpoint name.                                                 |
| `fqdn`                                                                                           | *string*                                                                                         | :heavy_check_mark:                                                                               | Fully qualified domain name.                                                                     |
| `issuedAt`                                                                                       | *string*                                                                                         | :heavy_minus_sign:                                                                               | ISO 8601 timestamp when certificate was issued (for renewal detection).                          |
| `privateKey`                                                                                     | *string*                                                                                         | :heavy_minus_sign:                                                                               | Decrypted private key (only present if status is "issued").                                      |