# CreateManagerResponseCustomDomains2

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { CreateManagerResponseCustomDomains2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseCustomDomains2 = {
  certificate: {},
  domain: "watery-coin.name",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                            | [models.CreateManagerResponseDomainsCertificate2](../models/createmanagerresponsedomainscertificate2.md) | :heavy_check_mark:                                                                                       | Platform-specific certificate references for custom domains.                                             |
| `domain`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Fully qualified domain name to use.                                                                      |