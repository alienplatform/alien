# ImportSourceDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { ImportSourceDomainsCertificate } from "@alienplatform/platform-api/models";

let value: ImportSourceDomainsCertificate = {};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `aws`                                       | *models.ImportSourceAwsUnion*               | :heavy_minus_sign:                          | N/A                                         |
| `azure`                                     | *models.ImportSourceAzureUnion*             | :heavy_minus_sign:                          | N/A                                         |
| `gcp`                                       | *models.ImportSourceGcpUnion*               | :heavy_minus_sign:                          | N/A                                         |
| `kubernetes`                                | *models.ImportSourceDomainsKubernetesUnion* | :heavy_minus_sign:                          | N/A                                         |