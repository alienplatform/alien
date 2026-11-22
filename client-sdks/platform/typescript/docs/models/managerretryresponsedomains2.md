# ManagerRetryResponseDomains2

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { ManagerRetryResponseDomains2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseDomains2 = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `customDomains`                                                                                              | Record<string, [models.ManagerRetryResponseCustomDomains2](../models/managerretryresponsecustomdomains2.md)> | :heavy_minus_sign:                                                                                           | Custom domain configuration per resource ID.                                                                 |
| `publicEndpointTarget`                                                                                       | *models.ManagerRetryResponsePublicEndpointTargetUnion2*                                                      | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |