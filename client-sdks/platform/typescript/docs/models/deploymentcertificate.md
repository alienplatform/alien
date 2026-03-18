# DeploymentCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { DeploymentCertificate } from "@alienplatform/platform-api/models";

let value: DeploymentCertificate = {};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `aws`                         | *models.DeploymentAwsUnion*   | :heavy_minus_sign:            | N/A                           |
| `azure`                       | *models.DeploymentAzureUnion* | :heavy_minus_sign:            | N/A                           |
| `gcp`                         | *models.DeploymentGcpUnion*   | :heavy_minus_sign:            | N/A                           |