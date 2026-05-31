# DomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { DomainsCertificate } from "@alienplatform/platform-api/models/operations";

let value: DomainsCertificate = {};
```

## Fields

| Field                               | Type                                | Required                            | Description                         |
| ----------------------------------- | ----------------------------------- | ----------------------------------- | ----------------------------------- |
| `aws`                               | *operations.Aws*                    | :heavy_minus_sign:                  | N/A                                 |
| `azure`                             | *operations.Azure*                  | :heavy_minus_sign:                  | N/A                                 |
| `gcp`                               | *operations.Gcp*                    | :heavy_minus_sign:                  | N/A                                 |
| `kubernetes`                        | *operations.DomainsKubernetesUnion* | :heavy_minus_sign:                  | N/A                                 |