# SyncReconcileResponseCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { SyncReconcileResponseCertificate } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseCertificate = {};
```

## Fields

| Field                                  | Type                                   | Required                               | Description                            |
| -------------------------------------- | -------------------------------------- | -------------------------------------- | -------------------------------------- |
| `aws`                                  | *models.SyncReconcileResponseAwsUnion* | :heavy_minus_sign:                     | N/A                                    |
| `azure`                                | *models.TargetAzureUnion*              | :heavy_minus_sign:                     | N/A                                    |
| `gcp`                                  | *models.TargetGcpUnion*                | :heavy_minus_sign:                     | N/A                                    |