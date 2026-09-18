# ImportSourceCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { ImportSourceCustomDomains } from "@alienplatform/platform-api/models";

let value: ImportSourceCustomDomains = {
  certificate: {},
  domain: "lively-baseboard.biz",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `certificate`                                                                        | [models.ImportSourceDomainsCertificate](../models/importsourcedomainscertificate.md) | :heavy_check_mark:                                                                   | Platform-specific certificate references for custom domains.                         |
| `domain`                                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | Fully qualified domain name to use.                                                  |