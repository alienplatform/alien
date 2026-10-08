# ManagerRetryResponseCustomDomains2

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { ManagerRetryResponseCustomDomains2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseCustomDomains2 = {
  certificate: {},
  domain: "optimal-executor.biz",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                          | [models.ManagerRetryResponseDomainsCertificate2](../models/managerretryresponsedomainscertificate2.md) | :heavy_check_mark:                                                                                     | Platform-specific certificate references for custom domains.                                           |
| `domain`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | Fully qualified domain name to use.                                                                    |