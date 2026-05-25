# SyncReconcileResponseCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { SyncReconcileResponseCertificate } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCertificate = {};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `aws`                                         | *models.SyncReconcileResponseDomainsAwsUnion* | :heavy_minus_sign:                            | N/A                                           |
| `azure`                                       | *models.DomainsTargetAzureUnion*              | :heavy_minus_sign:                            | N/A                                           |
| `gcp`                                         | *models.DomainsTargetGcpUnion*                | :heavy_minus_sign:                            | N/A                                           |