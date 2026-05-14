# PersistImportedDeploymentRequestCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestCertificate } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestCertificate = {};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `aws`                                               | *models.PersistImportedDeploymentRequestAwsUnion*   | :heavy_minus_sign:                                  | N/A                                                 |
| `azure`                                             | *models.PersistImportedDeploymentRequestAzureUnion* | :heavy_minus_sign:                                  | N/A                                                 |
| `gcp`                                               | *models.PersistImportedDeploymentRequestGcpUnion*   | :heavy_minus_sign:                                  | N/A                                                 |