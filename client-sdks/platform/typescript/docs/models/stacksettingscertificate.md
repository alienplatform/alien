# StackSettingsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { StackSettingsCertificate } from "@alienplatform/platform-api/models";

let value: StackSettingsCertificate = {};
```

## Fields

| Field                                        | Type                                         | Required                                     | Description                                  |
| -------------------------------------------- | -------------------------------------------- | -------------------------------------------- | -------------------------------------------- |
| `aws`                                        | *models.StackSettingsAwsUnion*               | :heavy_minus_sign:                           | N/A                                          |
| `azure`                                      | *models.StackSettingsAzureUnion*             | :heavy_minus_sign:                           | N/A                                          |
| `gcp`                                        | *models.StackSettingsGcpUnion*               | :heavy_minus_sign:                           | N/A                                          |
| `kubernetes`                                 | *models.StackSettingsDomainsKubernetesUnion* | :heavy_minus_sign:                           | N/A                                          |
| `managed`                                    | *models.StackSettingsManagedUnion*           | :heavy_minus_sign:                           | N/A                                          |