# CloudFormationCallbackRequestCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { CloudFormationCallbackRequestCustomDomains } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestCustomDomains = {
  certificate: {},
  domain: "messy-suv.biz",
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                          | [models.CloudFormationCallbackRequestDomainsCertificate](../models/cloudformationcallbackrequestdomainscertificate.md) | :heavy_check_mark:                                                                                                     | Platform-specific certificate references for custom domains.                                                           |
| `domain`                                                                                                               | *string*                                                                                                               | :heavy_check_mark:                                                                                                     | Fully qualified domain name to use.                                                                                    |