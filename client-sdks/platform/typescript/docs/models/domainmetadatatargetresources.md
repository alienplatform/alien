# DomainMetadataTargetResources

Certificate and DNS metadata for a public resource.

The direct fields describe the primary endpoint hostname. `endpoints`
contains endpoint-scoped metadata keyed by endpoint name. `aliases` contains
additional managed hostnames that route directly to the primary endpoint.

## Example Usage

```typescript
import { DomainMetadataTargetResources } from "@alienplatform/platform-api/models";

let value: DomainMetadataTargetResources = {
  certificateId: "<id>",
  certificateStatus: "renewing",
  dnsStatus: "updating",
  fqdn: "<value>",
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `aliases`                                                                                            | [models.SyncReconcileResponseAlias](../models/syncreconcileresponsealias.md)[]                       | :heavy_minus_sign:                                                                                   | Additional managed hostnames for the resource.                                                       |
| `certificateChain`                                                                                   | *string*                                                                                             | :heavy_minus_sign:                                                                                   | Full PEM certificate chain (only present if status is "issued").                                     |
| `certificateId`                                                                                      | *string*                                                                                             | :heavy_check_mark:                                                                                   | Certificate ID (for tracking/logging).                                                               |
| `certificateStatus`                                                                                  | [models.SyncReconcileResponseCertificateStatus](../models/syncreconcileresponsecertificatestatus.md) | :heavy_check_mark:                                                                                   | Certificate status in the certificate lifecycle                                                      |
| `dnsError`                                                                                           | *string*                                                                                             | :heavy_minus_sign:                                                                                   | Last DNS error message.                                                                              |
| `dnsStatus`                                                                                          | [models.SyncReconcileResponseDnsStatus](../models/syncreconcileresponsednsstatus.md)                 | :heavy_check_mark:                                                                                   | DNS record status in the DNS lifecycle                                                               |
| `endpoints`                                                                                          | Record<string, [models.SyncReconcileResponseEndpoints](../models/syncreconcileresponseendpoints.md)> | :heavy_minus_sign:                                                                                   | Endpoint-scoped metadata keyed by endpoint name.                                                     |
| `fqdn`                                                                                               | *string*                                                                                             | :heavy_check_mark:                                                                                   | Fully qualified domain name.                                                                         |
| `issuedAt`                                                                                           | *string*                                                                                             | :heavy_minus_sign:                                                                                   | ISO 8601 timestamp when certificate was issued (for renewal detection).                              |
| `privateKey`                                                                                         | *string*                                                                                             | :heavy_minus_sign:                                                                                   | Decrypted private key (only present if status is "issued").                                          |