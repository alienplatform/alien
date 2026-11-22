# ManagerRetryResponseCustomDomains1

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { ManagerRetryResponseCustomDomains1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCustomDomains1 = {
  certificate: {},
  domain: "corny-pillow.net",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                          | [models.ManagerRetryResponseDomainsCertificate1](../models/managerretryresponsedomainscertificate1.md) | :heavy_check_mark:                                                                                     | Platform-specific certificate references for custom domains.                                           |
| `domain`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | Fully qualified domain name to use.                                                                    |