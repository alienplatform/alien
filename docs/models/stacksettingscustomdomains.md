# StackSettingsCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { StackSettingsCustomDomains } from "@alienplatform/platform-api/models";

let value: StackSettingsCustomDomains = {
  certificate: {},
  domain: "stylish-window.name",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `certificate`                                                                          | [models.StackSettingsDomainsCertificate](../models/stacksettingsdomainscertificate.md) | :heavy_check_mark:                                                                     | Platform-specific certificate references for custom domains.                           |
| `domain`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | Fully qualified domain name to use.                                                    |