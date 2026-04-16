# SyncAcquireResponseDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { SyncAcquireResponseDomains } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDomains = {};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `customDomains`                                                                                          | Record<string, [models.SyncAcquireResponseCustomDomains](../models/syncacquireresponsecustomdomains.md)> | :heavy_minus_sign:                                                                                       | Custom domain configuration per resource ID.                                                             |