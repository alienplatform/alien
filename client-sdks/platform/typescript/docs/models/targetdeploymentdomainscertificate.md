# TargetDeploymentDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { TargetDeploymentDomainsCertificate } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDomainsCertificate = {};
```

## Fields

| Field                                           | Type                                            | Required                                        | Description                                     |
| ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- | ----------------------------------------------- |
| `aws`                                           | *models.TargetDeploymentStackSettingsAwsUnion*  | :heavy_minus_sign:                              | N/A                                             |
| `azure`                                         | *models.ConfigStackSettingsAzureUnion*          | :heavy_minus_sign:                              | N/A                                             |
| `gcp`                                           | *models.ConfigStackSettingsGcpUnion*            | :heavy_minus_sign:                              | N/A                                             |
| `kubernetes`                                    | *models.TargetDeploymentDomainsKubernetesUnion* | :heavy_minus_sign:                              | N/A                                             |