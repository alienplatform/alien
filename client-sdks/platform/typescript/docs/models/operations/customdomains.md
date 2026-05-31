# CustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { CustomDomains } from "@alienplatform/platform-api/models/operations";

let value: CustomDomains = {
  certificate: {},
  domain: "appropriate-mom.biz",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `certificate`                                                                  | [operations.DomainsCertificate](../../models/operations/domainscertificate.md) | :heavy_check_mark:                                                             | Platform-specific certificate references for custom domains.                   |
| `domain`                                                                       | *string*                                                                       | :heavy_check_mark:                                                             | Fully qualified domain name to use.                                            |