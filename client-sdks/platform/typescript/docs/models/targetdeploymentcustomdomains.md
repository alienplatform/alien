# TargetDeploymentCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { TargetDeploymentCustomDomains } from "@alienplatform/platform-api/models";

let value: TargetDeploymentCustomDomains = {
  certificate: {},
  domain: "ignorant-necklace.info",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `certificate`                                                                                | [models.TargetDeploymentDomainsCertificate](../models/targetdeploymentdomainscertificate.md) | :heavy_check_mark:                                                                           | Platform-specific certificate references for custom domains.                                 |
| `domain`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | Fully qualified domain name to use.                                                          |