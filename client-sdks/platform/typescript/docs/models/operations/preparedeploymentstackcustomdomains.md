# PrepareDeploymentStackCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { PrepareDeploymentStackCustomDomains } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackCustomDomains = {
  certificate: {},
  domain: "super-necklace.name",
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                              | [operations.PrepareDeploymentStackDomainsCertificate](../../models/operations/preparedeploymentstackdomainscertificate.md) | :heavy_check_mark:                                                                                                         | Platform-specific certificate references for custom domains.                                                               |
| `domain`                                                                                                                   | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Fully qualified domain name to use.                                                                                        |