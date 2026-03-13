# NewDeploymentRequestCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { NewDeploymentRequestCustomDomains } from "@aliendotdev/platform-api/models";

let value: NewDeploymentRequestCustomDomains = {
  certificate: {},
  domain: "whopping-tomatillo.biz",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `certificate`                                                                          | [models.NewDeploymentRequestCertificate](../models/newdeploymentrequestcertificate.md) | :heavy_check_mark:                                                                     | Platform-specific certificate references for custom domains.                           |
| `domain`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | Fully qualified domain name to use.                                                    |