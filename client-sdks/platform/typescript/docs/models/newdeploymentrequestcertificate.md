# NewDeploymentRequestCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { NewDeploymentRequestCertificate } from "@alienplatform/platform-api/models";

let value: NewDeploymentRequestCertificate = {};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `aws`                                   | *models.NewDeploymentRequestAwsUnion*   | :heavy_minus_sign:                      | N/A                                     |
| `azure`                                 | *models.NewDeploymentRequestAzureUnion* | :heavy_minus_sign:                      | N/A                                     |
| `gcp`                                   | *models.NewDeploymentRequestGcpUnion*   | :heavy_minus_sign:                      | N/A                                     |