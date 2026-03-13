# SyncReconcileResponseCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { SyncReconcileResponseCustomDomains } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseCustomDomains = {
  certificate: {},
  domain: "proud-hygienic.net",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `certificate`                                                                            | [models.SyncReconcileResponseCertificate](../models/syncreconcileresponsecertificate.md) | :heavy_check_mark:                                                                       | Platform-specific certificate references for custom domains.                             |
| `domain`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | Fully qualified domain name to use.                                                      |