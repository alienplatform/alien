# SyncListResponseCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { SyncListResponseCustomDomains } from "@alienplatform/platform-api/models";

let value: SyncListResponseCustomDomains = {
  certificate: {},
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `certificate`                                                                  | [models.SyncListResponseCertificate](../models/synclistresponsecertificate.md) | :heavy_check_mark:                                                             | Platform-specific certificate references for custom domains.                   |
| `domain`                                                                       | *string*                                                                       | :heavy_minus_sign:                                                             | Fully qualified domain name for a supplied certificate.                        |
| `endpoints`                                                                    | Record<string, *string*>                                                       | :heavy_minus_sign:                                                             | Hostnames keyed by endpoint name for a managed certificate.                    |