# CreateManagerResponseCustomDomains3

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { CreateManagerResponseCustomDomains3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCustomDomains3 = {
  certificate: {},
  domain: "direct-charm.com",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                            | [models.CreateManagerResponseDomainsCertificate3](../models/createmanagerresponsedomainscertificate3.md) | :heavy_check_mark:                                                                                       | Platform-specific certificate references for custom domains.                                             |
| `domain`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Fully qualified domain name to use.                                                                      |