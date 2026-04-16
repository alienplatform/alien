# DomainSettings

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { DomainSettings } from "@alienplatform/manager-api/models";

let value: DomainSettings = {};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `customDomains`                                                              | Record<string, [models.CustomDomainConfig](../models/customdomainconfig.md)> | :heavy_minus_sign:                                                           | Custom domain configuration per resource ID.                                 |