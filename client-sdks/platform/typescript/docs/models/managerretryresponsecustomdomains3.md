# ManagerRetryResponseCustomDomains3

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { ManagerRetryResponseCustomDomains3 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCustomDomains3 = {
  certificate: {},
  domain: "tempting-kick.org",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                          | [models.ManagerRetryResponseDomainsCertificate3](../models/managerretryresponsedomainscertificate3.md) | :heavy_check_mark:                                                                                     | Platform-specific certificate references for custom domains.                                           |
| `domain`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | Fully qualified domain name to use.                                                                    |