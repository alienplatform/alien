# CreateManagerResponseCertificate2

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { CreateManagerResponseCertificate2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCertificate2 = {};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `aws`                                                 | *models.CreateManagerResponseAwsUnion2*               | :heavy_minus_sign:                                    | N/A                                                   |
| `azure`                                               | *models.CreateManagerResponseAzureUnion2*             | :heavy_minus_sign:                                    | N/A                                                   |
| `gcp`                                                 | *models.CreateManagerResponseGcpUnion2*               | :heavy_minus_sign:                                    | N/A                                                   |
| `kubernetes`                                          | *models.CreateManagerResponseDomainsKubernetesUnion2* | :heavy_minus_sign:                                    | N/A                                                   |
| `managed`                                             | *models.CreateManagerResponseManagedUnion2*           | :heavy_minus_sign:                                    | N/A                                                   |