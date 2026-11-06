# StackSettingsCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { StackSettingsCustomDomains } from "@alienplatform/platform-api/models";

let value: StackSettingsCustomDomains = {
  certificate: {},
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `certificate`                                                            | [models.StackSettingsCertificate](../models/stacksettingscertificate.md) | :heavy_check_mark:                                                       | Platform-specific certificate references for custom domains.             |
| `domain`                                                                 | *string*                                                                 | :heavy_minus_sign:                                                       | Fully qualified domain name for a supplied certificate.                  |
| `endpoints`                                                              | Record<string, *string*>                                                 | :heavy_minus_sign:                                                       | Hostnames keyed by endpoint name for a managed certificate.              |