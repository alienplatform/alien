# CreateManagerResponseDomainsCertificate2

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { CreateManagerResponseDomainsCertificate2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseDomainsCertificate2 = {};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `aws`                                                 | *models.CreateManagerResponseAwsUnion2*               | :heavy_minus_sign:                                    | N/A                                                   |
| `azure`                                               | *models.CreateManagerResponseAzureUnion2*             | :heavy_minus_sign:                                    | N/A                                                   |
| `gcp`                                                 | *models.CreateManagerResponseGcpUnion2*               | :heavy_minus_sign:                                    | N/A                                                   |
| `kubernetes`                                          | *models.CreateManagerResponseDomainsKubernetesUnion2* | :heavy_minus_sign:                                    | N/A                                                   |