# CreateManagerResponseCustomDomains1

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { CreateManagerResponseCustomDomains1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCustomDomains1 = {
  certificate: {},
  domain: "beneficial-celsius.info",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                            | [models.CreateManagerResponseDomainsCertificate1](../models/createmanagerresponsedomainscertificate1.md) | :heavy_check_mark:                                                                                       | Platform-specific certificate references for custom domains.                                             |
| `domain`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Fully qualified domain name to use.                                                                      |