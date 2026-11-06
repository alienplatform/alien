# ManagerRetryResponseCertificate1

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { ManagerRetryResponseCertificate1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCertificate1 = {};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `aws`                                                | *models.ManagerRetryResponseAwsUnion1*               | :heavy_minus_sign:                                   | N/A                                                  |
| `azure`                                              | *models.ManagerRetryResponseAzureUnion1*             | :heavy_minus_sign:                                   | N/A                                                  |
| `gcp`                                                | *models.ManagerRetryResponseGcpUnion1*               | :heavy_minus_sign:                                   | N/A                                                  |
| `kubernetes`                                         | *models.ManagerRetryResponseDomainsKubernetesUnion1* | :heavy_minus_sign:                                   | N/A                                                  |
| `managed`                                            | *models.ManagerRetryResponseManagedUnion1*           | :heavy_minus_sign:                                   | N/A                                                  |