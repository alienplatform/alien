# SyncReconcileResponseDomains

Domain configuration for the stack.

When `custom_domains` is set, the specified resources use customer-provided
domains and certificates. Otherwise, Alien auto-generates domains.

## Example Usage

```typescript
import { SyncReconcileResponseDomains } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseDomains = {};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `customDomains`                                                                                              | Record<string, [models.SyncReconcileResponseCustomDomains](../models/syncreconcileresponsecustomdomains.md)> | :heavy_minus_sign:                                                                                           | Custom domain configuration per resource ID.                                                                 |