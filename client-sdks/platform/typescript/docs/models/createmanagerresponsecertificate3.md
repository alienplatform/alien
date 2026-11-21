# CreateManagerResponseCertificate3

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { CreateManagerResponseCertificate3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCertificate3 = {};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `aws`                                                 | *models.CreateManagerResponseAwsUnion3*               | :heavy_minus_sign:                                    | N/A                                                   |
| `azure`                                               | *models.CreateManagerResponseAzureUnion3*             | :heavy_minus_sign:                                    | N/A                                                   |
| `gcp`                                                 | *models.CreateManagerResponseGcpUnion3*               | :heavy_minus_sign:                                    | N/A                                                   |
| `kubernetes`                                          | *models.CreateManagerResponseDomainsKubernetesUnion3* | :heavy_minus_sign:                                    | N/A                                                   |
| `managed`                                             | *models.CreateManagerResponseManagedUnion3*           | :heavy_minus_sign:                                    | N/A                                                   |