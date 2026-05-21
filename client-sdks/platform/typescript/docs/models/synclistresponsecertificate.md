# SyncListResponseCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { SyncListResponseCertificate } from "@alienplatform/platform-api/models";

let value: SyncListResponseCertificate = {};
```

## Fields

| Field                               | Type                                | Required                            | Description                         |
| ----------------------------------- | ----------------------------------- | ----------------------------------- | ----------------------------------- |
| `aws`                               | *models.SyncListResponseAwsUnion*   | :heavy_minus_sign:                  | N/A                                 |
| `azure`                             | *models.SyncListResponseAzureUnion* | :heavy_minus_sign:                  | N/A                                 |
| `gcp`                               | *models.SyncListResponseGcpUnion*   | :heavy_minus_sign:                  | N/A                                 |