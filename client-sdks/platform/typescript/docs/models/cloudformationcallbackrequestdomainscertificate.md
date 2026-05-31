# CloudFormationCallbackRequestDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { CloudFormationCallbackRequestDomainsCertificate } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestDomainsCertificate = {};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `aws`                                                        | *models.CloudFormationCallbackRequestAwsUnion*               | :heavy_minus_sign:                                           | N/A                                                          |
| `azure`                                                      | *models.CloudFormationCallbackRequestAzureUnion*             | :heavy_minus_sign:                                           | N/A                                                          |
| `gcp`                                                        | *models.CloudFormationCallbackRequestGcpUnion*               | :heavy_minus_sign:                                           | N/A                                                          |
| `kubernetes`                                                 | *models.CloudFormationCallbackRequestDomainsKubernetesUnion* | :heavy_minus_sign:                                           | N/A                                                          |