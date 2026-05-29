# SyncReconcileResponseDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { SyncReconcileResponseDomainsCertificate } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDomainsCertificate = {};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `aws`                                                | *models.SyncReconcileResponseStackSettingsAwsUnion*  | :heavy_minus_sign:                                   | N/A                                                  |
| `azure`                                              | *models.TargetStackSettingsAzureUnion*               | :heavy_minus_sign:                                   | N/A                                                  |
| `gcp`                                                | *models.TargetStackSettingsGcpUnion*                 | :heavy_minus_sign:                                   | N/A                                                  |
| `kubernetes`                                         | *models.SyncReconcileResponseDomainsKubernetesUnion* | :heavy_minus_sign:                                   | N/A                                                  |