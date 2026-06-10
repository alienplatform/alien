# CreateSetupRegistrationOperationRequestCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestCustomDomains } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestCustomDomains = {
  certificate: {},
  domain: "wrong-tuba.biz",
};
```

## Fields

| Field                                                                                                                                      | Type                                                                                                                                       | Required                                                                                                                                   | Description                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                                                              | [models.CreateSetupRegistrationOperationRequestDomainsCertificate](../models/createsetupregistrationoperationrequestdomainscertificate.md) | :heavy_check_mark:                                                                                                                         | Platform-specific certificate references for custom domains.                                                                               |
| `domain`                                                                                                                                   | *string*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | Fully qualified domain name to use.                                                                                                        |