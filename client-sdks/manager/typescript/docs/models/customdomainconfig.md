# CustomDomainConfig

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { CustomDomainConfig } from "@alienplatform/manager-api/models";

let value: CustomDomainConfig = {
  certificate: {},
  domain: "queasy-accelerator.net",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `certificate`                                                          | [models.CustomCertificateConfig](../models/customcertificateconfig.md) | :heavy_check_mark:                                                     | Platform-specific certificate references for custom domains.           |
| `domain`                                                               | *string*                                                               | :heavy_check_mark:                                                     | Fully qualified domain name to use.                                    |