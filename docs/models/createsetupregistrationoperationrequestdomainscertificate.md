# CreateSetupRegistrationOperationRequestDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestDomainsCertificate } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestDomainsCertificate = {};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `aws`                                                                  | *models.CreateSetupRegistrationOperationRequestAwsUnion*               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `azure`                                                                | *models.CreateSetupRegistrationOperationRequestAzureUnion*             | :heavy_minus_sign:                                                     | N/A                                                                    |
| `gcp`                                                                  | *models.CreateSetupRegistrationOperationRequestGcpUnion*               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `kubernetes`                                                           | *models.CreateSetupRegistrationOperationRequestDomainsKubernetesUnion* | :heavy_minus_sign:                                                     | N/A                                                                    |