# SyncAcquireResponseCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { SyncAcquireResponseCertificate } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseCertificate = {};
```

## Fields

| Field                                  | Type                                   | Required                               | Description                            |
| -------------------------------------- | -------------------------------------- | -------------------------------------- | -------------------------------------- |
| `aws`                                  | *models.SyncAcquireResponseAwsUnion*   | :heavy_minus_sign:                     | N/A                                    |
| `azure`                                | *models.SyncAcquireResponseAzureUnion* | :heavy_minus_sign:                     | N/A                                    |
| `gcp`                                  | *models.SyncAcquireResponseGcpUnion*   | :heavy_minus_sign:                     | N/A                                    |