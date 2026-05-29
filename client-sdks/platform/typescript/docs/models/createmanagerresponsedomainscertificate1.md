# CreateManagerResponseDomainsCertificate1

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { CreateManagerResponseDomainsCertificate1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseDomainsCertificate1 = {};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `aws`                                                 | *models.CreateManagerResponseAwsUnion1*               | :heavy_minus_sign:                                    | N/A                                                   |
| `azure`                                               | *models.CreateManagerResponseAzureUnion1*             | :heavy_minus_sign:                                    | N/A                                                   |
| `gcp`                                                 | *models.CreateManagerResponseGcpUnion1*               | :heavy_minus_sign:                                    | N/A                                                   |
| `kubernetes`                                          | *models.CreateManagerResponseDomainsKubernetesUnion1* | :heavy_minus_sign:                                    | N/A                                                   |