# DeploymentCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { DeploymentCustomDomains } from "@aliendotdev/platform-api/models";

let value: DeploymentCustomDomains = {
  certificate: {},
  domain: "genuine-rust.net",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `certificate`                                                      | [models.DeploymentCertificate](../models/deploymentcertificate.md) | :heavy_check_mark:                                                 | Platform-specific certificate references for custom domains.       |
| `domain`                                                           | *string*                                                           | :heavy_check_mark:                                                 | Fully qualified domain name to use.                                |