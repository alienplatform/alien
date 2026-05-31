# CreateManagerResponseDomains1

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { CreateManagerResponseDomains1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseDomains1 = {};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `customDomains`                                                                                                | Record<string, [models.CreateManagerResponseCustomDomains1](../models/createmanagerresponsecustomdomains1.md)> | :heavy_minus_sign:                                                                                             | Custom domain configuration per resource ID.                                                                   |