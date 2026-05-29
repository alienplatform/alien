# SyncListResponseCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { SyncListResponseCustomDomains } from "@alienplatform/platform-api/models";

let value: SyncListResponseCustomDomains = {
  certificate: {},
  domain: "shrill-stock.info",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `certificate`                                                                                | [models.SyncListResponseDomainsCertificate](../models/synclistresponsedomainscertificate.md) | :heavy_check_mark:                                                                           | Platform-specific certificate references for custom domains.                                 |
| `domain`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | Fully qualified domain name to use.                                                          |