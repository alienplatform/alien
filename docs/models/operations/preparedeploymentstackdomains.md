# PrepareDeploymentStackDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { PrepareDeploymentStackDomains } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackDomains = {};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `customDomains`                                                                                                                  | Record<string, [operations.PrepareDeploymentStackCustomDomains](../../models/operations/preparedeploymentstackcustomdomains.md)> | :heavy_minus_sign:                                                                                                               | Custom domain configuration per resource ID.                                                                                     |
| `publicEndpointTarget`                                                                                                           | *operations.PrepareDeploymentStackPublicEndpointTargetUnion*                                                                     | :heavy_minus_sign:                                                                                                               | N/A                                                                                                                              |