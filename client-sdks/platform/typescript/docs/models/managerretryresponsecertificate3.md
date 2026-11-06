# ManagerRetryResponseCertificate3

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { ManagerRetryResponseCertificate3 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCertificate3 = {};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `aws`                                                | *models.ManagerRetryResponseAwsUnion3*               | :heavy_minus_sign:                                   | N/A                                                  |
| `azure`                                              | *models.ManagerRetryResponseAzureUnion3*             | :heavy_minus_sign:                                   | N/A                                                  |
| `gcp`                                                | *models.ManagerRetryResponseGcpUnion3*               | :heavy_minus_sign:                                   | N/A                                                  |
| `kubernetes`                                         | *models.ManagerRetryResponseDomainsKubernetesUnion3* | :heavy_minus_sign:                                   | N/A                                                  |
| `managed`                                            | *models.ManagerRetryResponseManagedUnion3*           | :heavy_minus_sign:                                   | N/A                                                  |