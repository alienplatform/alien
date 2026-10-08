# StackSettingsDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { StackSettingsDomainsCertificate } from "@alienplatform/platform-api/models";

let value: StackSettingsDomainsCertificate = {};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `aws`                                        | *models.StackSettingsAwsUnion*               | :heavy_minus_sign:                           | N/A                                          |
| `azure`                                      | *models.StackSettingsAzureUnion*             | :heavy_minus_sign:                           | N/A                                          |
| `gcp`                                        | *models.StackSettingsGcpUnion*               | :heavy_minus_sign:                           | N/A                                          |
| `kubernetes`                                 | *models.StackSettingsDomainsKubernetesUnion* | :heavy_minus_sign:                           | N/A                                          |