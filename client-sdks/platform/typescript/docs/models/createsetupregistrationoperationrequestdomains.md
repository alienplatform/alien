# CreateSetupRegistrationOperationRequestDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestDomains } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestDomains = {};
```

## Fields

| Field                                                                                                                                            | Type                                                                                                                                             | Required                                                                                                                                         | Description                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `customDomains`                                                                                                                                  | Record<string, [models.CreateSetupRegistrationOperationRequestCustomDomains](../models/createsetupregistrationoperationrequestcustomdomains.md)> | :heavy_minus_sign:                                                                                                                               | Custom domain configuration per resource ID.                                                                                                     |