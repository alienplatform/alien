# SyncAcquireResponseDeploymentDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDomainsCertificate } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDomainsCertificate = {};
```

## Fields

| Field                                                         | Type                                                          | Required                                                      | Description                                                   |
| ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- | ------------------------------------------------------------- |
| `aws`                                                         | *models.SyncAcquireResponseDeploymentStackSettingsAwsUnion*   | :heavy_minus_sign:                                            | N/A                                                           |
| `azure`                                                       | *models.SyncAcquireResponseDeploymentStackSettingsAzureUnion* | :heavy_minus_sign:                                            | N/A                                                           |
| `gcp`                                                         | *models.SyncAcquireResponseDeploymentStackSettingsGcpUnion*   | :heavy_minus_sign:                                            | N/A                                                           |
| `kubernetes`                                                  | *models.SyncAcquireResponseDeploymentDomainsKubernetesUnion*  | :heavy_minus_sign:                                            | N/A                                                           |