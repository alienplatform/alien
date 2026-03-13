# SyncAcquireResponseCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { SyncAcquireResponseCustomDomains } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseCustomDomains = {
  certificate: {},
  domain: "outstanding-pronoun.biz",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `certificate`                                                                        | [models.SyncAcquireResponseCertificate](../models/syncacquireresponsecertificate.md) | :heavy_check_mark:                                                                   | Platform-specific certificate references for custom domains.                         |
| `domain`                                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | Fully qualified domain name to use.                                                  |