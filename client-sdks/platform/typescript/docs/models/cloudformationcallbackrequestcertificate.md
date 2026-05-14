# CloudFormationCallbackRequestCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { CloudFormationCallbackRequestCertificate } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestCertificate = {};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `aws`                                            | *models.CloudFormationCallbackRequestAwsUnion*   | :heavy_minus_sign:                               | N/A                                              |
| `azure`                                          | *models.CloudFormationCallbackRequestAzureUnion* | :heavy_minus_sign:                               | N/A                                              |
| `gcp`                                            | *models.CloudFormationCallbackRequestGcpUnion*   | :heavy_minus_sign:                               | N/A                                              |