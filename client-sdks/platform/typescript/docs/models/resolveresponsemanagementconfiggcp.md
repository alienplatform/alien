# ResolveResponseManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { ResolveResponseManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: ResolveResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `federationTrust`                                                  | *models.ResolveResponseFederationTrustUnion*                       | :heavy_minus_sign:                                                 | N/A                                                                |
| `serviceAccountEmail`                                              | *string*                                                           | :heavy_check_mark:                                                 | Linked source account metadata; never a direct target trust grant. |
| `platform`                                                         | *"gcp"*                                                            | :heavy_check_mark:                                                 | N/A                                                                |