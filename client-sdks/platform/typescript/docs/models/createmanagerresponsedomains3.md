# CreateManagerResponseDomains3

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { CreateManagerResponseDomains3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseDomains3 = {};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `customDomains`                                                                                                | Record<string, [models.CreateManagerResponseCustomDomains3](../models/createmanagerresponsecustomdomains3.md)> | :heavy_minus_sign:                                                                                             | Custom domain configuration per resource ID.                                                                   |
| `publicEndpointTarget`                                                                                         | *models.CreateManagerResponsePublicEndpointTargetUnion3*                                                       | :heavy_minus_sign:                                                                                             | N/A                                                                                                            |