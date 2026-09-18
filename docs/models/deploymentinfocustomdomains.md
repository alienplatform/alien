# DeploymentInfoCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { DeploymentInfoCustomDomains } from "@alienplatform/platform-api/models";

let value: DeploymentInfoCustomDomains = {
  certificate: {},
  domain: "close-stitcher.biz",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `certificate`                                                                            | [models.DeploymentInfoDomainsCertificate](../models/deploymentinfodomainscertificate.md) | :heavy_check_mark:                                                                       | Platform-specific certificate references for custom domains.                             |
| `domain`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | Fully qualified domain name to use.                                                      |