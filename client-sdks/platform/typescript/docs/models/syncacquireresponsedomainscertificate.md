# SyncAcquireResponseDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { SyncAcquireResponseDomainsCertificate } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDomainsCertificate = {};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `aws`                                               | *models.SyncAcquireResponseStackSettingsAwsUnion*   | :heavy_minus_sign:                                  | N/A                                                 |
| `azure`                                             | *models.SyncAcquireResponseStackSettingsAzureUnion* | :heavy_minus_sign:                                  | N/A                                                 |
| `gcp`                                               | *models.SyncAcquireResponseStackSettingsGcpUnion*   | :heavy_minus_sign:                                  | N/A                                                 |
| `kubernetes`                                        | *models.SyncAcquireResponseDomainsKubernetesUnion*  | :heavy_minus_sign:                                  | N/A                                                 |