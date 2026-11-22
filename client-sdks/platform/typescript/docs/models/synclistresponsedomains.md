# SyncListResponseDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { SyncListResponseDomains } from "@alienplatform/platform-api/models";

let value: SyncListResponseDomains = {};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `customDomains`                                                                                    | Record<string, [models.SyncListResponseCustomDomains](../models/synclistresponsecustomdomains.md)> | :heavy_minus_sign:                                                                                 | Custom domain configuration per resource ID.                                                       |
| `publicEndpointTarget`                                                                             | *models.SyncListResponsePublicEndpointTargetUnion*                                                 | :heavy_minus_sign:                                                                                 | N/A                                                                                                |