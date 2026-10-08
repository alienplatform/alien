# DeploymentConfigCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { DeploymentConfigCustomDomains } from "@alienplatform/platform-api/models";

let value: DeploymentConfigCustomDomains = {
  certificate: {},
  domain: "animated-produce.com",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `certificate`                                                                                | [models.DeploymentConfigDomainsCertificate](../models/deploymentconfigdomainscertificate.md) | :heavy_check_mark:                                                                           | Platform-specific certificate references for custom domains.                                 |
| `domain`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | Fully qualified domain name to use.                                                          |