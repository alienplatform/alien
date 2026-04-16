# CustomCertificateConfig

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { CustomCertificateConfig } from "@alienplatform/manager-api/models";

let value: CustomCertificateConfig = {};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `aws`                                                                            | [models.AwsCustomCertificateConfig](../models/awscustomcertificateconfig.md)     | :heavy_minus_sign:                                                               | N/A                                                                              |
| `azure`                                                                          | [models.AzureCustomCertificateConfig](../models/azurecustomcertificateconfig.md) | :heavy_minus_sign:                                                               | N/A                                                                              |
| `gcp`                                                                            | [models.GcpCustomCertificateConfig](../models/gcpcustomcertificateconfig.md)     | :heavy_minus_sign:                                                               | N/A                                                                              |