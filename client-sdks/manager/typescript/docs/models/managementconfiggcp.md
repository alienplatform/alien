# ManagementConfigGcp

GCP management configuration

## Example Usage

```typescript
import { ManagementConfigGcp } from "@alienplatform/manager-api/models";

let value: ManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `federationTrust`                                                  | [models.GcpFederationTrust](../models/gcpfederationtrust.md)       | :heavy_minus_sign:                                                 | N/A                                                                |
| `serviceAccountEmail`                                              | *string*                                                           | :heavy_check_mark:                                                 | Linked source account metadata; never a direct target trust grant. |
| `platform`                                                         | *"gcp"*                                                            | :heavy_check_mark:                                                 | N/A                                                                |
