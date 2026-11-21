# StackSettingsDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { StackSettingsDomains } from "@alienplatform/platform-api/models";

let value: StackSettingsDomains = {};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `customDomains`                                                                              | Record<string, [models.StackSettingsCustomDomains](../models/stacksettingscustomdomains.md)> | :heavy_minus_sign:                                                                           | Custom domain configuration per resource ID.                                                 |
| `publicEndpointTarget`                                                                       | *models.StackSettingsPublicEndpointTargetUnion*                                              | :heavy_minus_sign:                                                                           | N/A                                                                                          |