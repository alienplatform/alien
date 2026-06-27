# PlanDeploymentComputeCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { PlanDeploymentComputeCustomDomains } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeCustomDomains = {
  certificate: {},
  domain: "babyish-encouragement.biz",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                                            | [operations.PlanDeploymentComputeDomainsCertificate](../../models/operations/plandeploymentcomputedomainscertificate.md) | :heavy_check_mark:                                                                                                       | Platform-specific certificate references for custom domains.                                                             |
| `domain`                                                                                                                 | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | Fully qualified domain name to use.                                                                                      |