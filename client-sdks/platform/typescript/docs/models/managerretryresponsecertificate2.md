# ManagerRetryResponseCertificate2

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { ManagerRetryResponseCertificate2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCertificate2 = {};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `aws`                                                | *models.ManagerRetryResponseAwsUnion2*               | :heavy_minus_sign:                                   | N/A                                                  |
| `azure`                                              | *models.ManagerRetryResponseAzureUnion2*             | :heavy_minus_sign:                                   | N/A                                                  |
| `gcp`                                                | *models.ManagerRetryResponseGcpUnion2*               | :heavy_minus_sign:                                   | N/A                                                  |
| `kubernetes`                                         | *models.ManagerRetryResponseDomainsKubernetesUnion2* | :heavy_minus_sign:                                   | N/A                                                  |
| `managed`                                            | *models.ManagerRetryResponseManagedUnion2*           | :heavy_minus_sign:                                   | N/A                                                  |