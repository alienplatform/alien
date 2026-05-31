# ManagerRetryResponseDomains1

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { ManagerRetryResponseDomains1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseDomains1 = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `customDomains`                                                                                              | Record<string, [models.ManagerRetryResponseCustomDomains1](../models/managerretryresponsecustomdomains1.md)> | :heavy_minus_sign:                                                                                           | Custom domain configuration per resource ID.                                                                 |