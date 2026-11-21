# DeploymentInfoCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { DeploymentInfoCertificate } from "@alienplatform/platform-api/models";

let value: DeploymentInfoCertificate = {};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `aws`                                         | *models.DeploymentInfoAwsUnion*               | :heavy_minus_sign:                            | N/A                                           |
| `azure`                                       | *models.DeploymentInfoAzureUnion*             | :heavy_minus_sign:                            | N/A                                           |
| `gcp`                                         | *models.DeploymentInfoGcpUnion*               | :heavy_minus_sign:                            | N/A                                           |
| `kubernetes`                                  | *models.DeploymentInfoDomainsKubernetesUnion* | :heavy_minus_sign:                            | N/A                                           |
| `managed`                                     | *models.DeploymentInfoManagedUnion*           | :heavy_minus_sign:                            | N/A                                           |