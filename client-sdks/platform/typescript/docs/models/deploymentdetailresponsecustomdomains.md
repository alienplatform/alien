# DeploymentDetailResponseCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { DeploymentDetailResponseCustomDomains } from "@aliendotdev/platform-api/models";

let value: DeploymentDetailResponseCustomDomains = {
  certificate: {},
  domain: "scientific-pick.com",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `certificate`                                                                                  | [models.DeploymentDetailResponseCertificate](../models/deploymentdetailresponsecertificate.md) | :heavy_check_mark:                                                                             | Platform-specific certificate references for custom domains.                                   |
| `domain`                                                                                       | *string*                                                                                       | :heavy_check_mark:                                                                             | Fully qualified domain name to use.                                                            |