# PlanDeploymentComputeCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { PlanDeploymentComputeCustomDomains } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeCustomDomains = {
  certificate: {},
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                              | [operations.PlanDeploymentComputeCertificate](../../models/operations/plandeploymentcomputecertificate.md) | :heavy_check_mark:                                                                                         | Platform-specific certificate references for custom domains.                                               |
| `domain`                                                                                                   | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | Fully qualified domain name for a supplied certificate.                                                    |
| `endpoints`                                                                                                | Record<string, *string*>                                                                                   | :heavy_minus_sign:                                                                                         | Hostnames keyed by endpoint name for a managed certificate.                                                |