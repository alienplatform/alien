# Domains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { Domains } from "@alienplatform/platform-api/models/operations";

let value: Domains = {};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `customDomains`                                                                      | Record<string, [operations.CustomDomains](../../models/operations/customdomains.md)> | :heavy_minus_sign:                                                                   | Custom domain configuration per resource ID.                                         |