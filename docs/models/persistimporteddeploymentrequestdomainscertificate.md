# PersistImportedDeploymentRequestDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestDomainsCertificate } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestDomainsCertificate = {};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `aws`                                                           | *models.PersistImportedDeploymentRequestAwsUnion*               | :heavy_minus_sign:                                              | N/A                                                             |
| `azure`                                                         | *models.PersistImportedDeploymentRequestAzureUnion*             | :heavy_minus_sign:                                              | N/A                                                             |
| `gcp`                                                           | *models.PersistImportedDeploymentRequestGcpUnion*               | :heavy_minus_sign:                                              | N/A                                                             |
| `kubernetes`                                                    | *models.PersistImportedDeploymentRequestDomainsKubernetesUnion* | :heavy_minus_sign:                                              | N/A                                                             |