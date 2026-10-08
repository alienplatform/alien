# ManagerRetryResponseDomains3

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { ManagerRetryResponseDomains3 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseDomains3 = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `customDomains`                                                                                              | Record<string, [models.ManagerRetryResponseCustomDomains3](../models/managerretryresponsecustomdomains3.md)> | :heavy_minus_sign:                                                                                           | Custom domain configuration per resource ID.                                                                 |
| `publicEndpointTarget`                                                                                       | *models.ManagerRetryResponsePublicEndpointTargetUnion3*                                                      | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |