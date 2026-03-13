# DeploymentDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { DeploymentDomains } from "@alienplatform/platform-api/models";

let value: DeploymentDomains = {};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `customDomains`                                                                        | Record<string, [models.DeploymentCustomDomains](../models/deploymentcustomdomains.md)> | :heavy_minus_sign:                                                                     | Custom domain configuration per resource ID.                                           |