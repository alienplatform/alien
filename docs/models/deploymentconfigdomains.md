# DeploymentConfigDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { DeploymentConfigDomains } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDomains = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `customDomains`                                                                                    | Record<string, [models.DeploymentConfigCustomDomains](../models/deploymentconfigcustomdomains.md)> | :heavy_minus_sign:                                                                                 | Custom domain configuration per resource ID.                                                       |
| `publicEndpointTarget`                                                                             | *models.DeploymentConfigPublicEndpointTargetUnion*                                                 | :heavy_minus_sign:                                                                                 | N/A                                                                                                |