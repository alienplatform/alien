# PlanDeploymentComputeDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { PlanDeploymentComputeDomains } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeDomains = {};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `customDomains`                                                                                                                | Record<string, [operations.PlanDeploymentComputeCustomDomains](../../models/operations/plandeploymentcomputecustomdomains.md)> | :heavy_minus_sign:                                                                                                             | Custom domain configuration per resource ID.                                                                                   |
| `publicEndpointTarget`                                                                                                         | *operations.PlanDeploymentComputePublicEndpointTargetUnion*                                                                    | :heavy_minus_sign:                                                                                                             | N/A                                                                                                                            |