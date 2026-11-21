# DeploymentConfigResources

Certificate and DNS metadata for a public resource.

The direct fields describe the primary endpoint hostname. `endpoints`
contains endpoint-scoped metadata keyed by endpoint name. `aliases` contains
additional managed hostnames that route directly to the primary endpoint.

## Example Usage

```typescript
import { DeploymentConfigResources } from "@alienplatform/platform-api/models";

let value: DeploymentConfigResources = {
  certificateId: "<id>",
  certificateStatus: "renewing",
  dnsStatus: "pending",
  fqdn: "<value>",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `aliases`                                                                                  | [models.DeploymentConfigAlias](../models/deploymentconfigalias.md)[]                       | :heavy_minus_sign:                                                                         | Additional managed hostnames for the resource.                                             |
| `certificateChain`                                                                         | *string*                                                                                   | :heavy_minus_sign:                                                                         | Full PEM certificate chain (only present if status is "issued").                           |
| `certificateId`                                                                            | *string*                                                                                   | :heavy_check_mark:                                                                         | Certificate ID (for tracking/logging).                                                     |
| `certificateStatus`                                                                        | [models.DeploymentConfigCertificateStatus](../models/deploymentconfigcertificatestatus.md) | :heavy_check_mark:                                                                         | Certificate status in the certificate lifecycle                                            |
| `dnsError`                                                                                 | *string*                                                                                   | :heavy_minus_sign:                                                                         | Last DNS error message.                                                                    |
| `dnsStatus`                                                                                | [models.DeploymentConfigDnsStatus](../models/deploymentconfigdnsstatus.md)                 | :heavy_check_mark:                                                                         | DNS record status in the DNS lifecycle                                                     |
| `endpoints`                                                                                | Record<string, [models.DeploymentConfigEndpoints](../models/deploymentconfigendpoints.md)> | :heavy_minus_sign:                                                                         | Endpoint-scoped metadata keyed by endpoint name.                                           |
| `fqdn`                                                                                     | *string*                                                                                   | :heavy_check_mark:                                                                         | Fully qualified domain name.                                                               |
| `issuedAt`                                                                                 | *string*                                                                                   | :heavy_minus_sign:                                                                         | ISO 8601 timestamp when certificate was issued (for renewal detection).                    |
| `privateKey`                                                                               | *string*                                                                                   | :heavy_minus_sign:                                                                         | Decrypted private key (only present if status is "issued").                                |